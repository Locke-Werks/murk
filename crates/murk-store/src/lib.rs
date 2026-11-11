//! Persistent message log with full-text search, scrollback paging and read markers.

use murk_core::{Line, LineKind};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub type Result<T> = rusqlite::Result<T>;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS messages (
    id        INTEGER PRIMARY KEY,
    network   TEXT NOT NULL,
    buffer    TEXT NOT NULL,
    kind      TEXT NOT NULL,
    extra     TEXT,
    sender    TEXT,
    text      TEXT NOT NULL,
    time_ms   INTEGER NOT NULL,
    msgid     TEXT,
    own       INTEGER NOT NULL DEFAULT 0,
    highlight INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS messages_buffer ON messages(network, buffer, time_ms, id);
CREATE UNIQUE INDEX IF NOT EXISTS messages_msgid ON messages(network, buffer, msgid) WHERE msgid IS NOT NULL;
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(text, sender, content='messages', content_rowid='id');
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, text, sender) VALUES (new.id, new.text, new.sender);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, text, sender) VALUES ('delete', old.id, old.text, old.sender);
END;
CREATE TABLE IF NOT EXISTS read_markers (
    network TEXT NOT NULL,
    buffer  TEXT NOT NULL,
    time_ms INTEGER NOT NULL,
    PRIMARY KEY (network, buffer)
);
";

/// The server buffer is stored under this name.
pub const SERVER_BUFFER: &str = "*server*";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub id: i64,
    pub network: String,
    pub buffer: String,
    pub kind: LineKind,
    pub sender: Option<String>,
    pub text: String,
    pub time: SystemTime,
    pub msgid: Option<String>,
    pub own: bool,
    pub highlight: bool,
}

pub struct Store {
    conn: Connection,
}

fn to_ms(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn from_ms(ms: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64)
}

/// Kind name plus an optional extra field (reason, kick target or new nick).
fn encode_kind(k: &LineKind) -> (&'static str, Option<String>) {
    match k {
        LineKind::Privmsg => ("privmsg", None),
        LineKind::Notice => ("notice", None),
        LineKind::Action => ("action", None),
        LineKind::Join => ("join", None),
        LineKind::Part { reason } => ("part", reason.clone()),
        LineKind::Quit { reason } => ("quit", reason.clone()),
        LineKind::Kick { target, reason } => (
            "kick",
            Some(match reason {
                Some(r) => format!("{target} {r}"),
                None => target.clone(),
            }),
        ),
        LineKind::Nick { new } => ("nick", Some(new.clone())),
        LineKind::Topic => ("topic", None),
        LineKind::Mode => ("mode", None),
        LineKind::Server => ("server", None),
        LineKind::Error => ("error", None),
    }
}

fn decode_kind(kind: &str, extra: Option<String>) -> LineKind {
    match kind {
        "notice" => LineKind::Notice,
        "action" => LineKind::Action,
        "join" => LineKind::Join,
        "part" => LineKind::Part { reason: extra },
        "quit" => LineKind::Quit { reason: extra },
        "kick" => {
            let e = extra.unwrap_or_default();
            let (target, reason) = match e.split_once(' ') {
                Some((t, r)) => (t.to_string(), Some(r.to_string())),
                None => (e, None),
            };
            LineKind::Kick { target, reason }
        }
        "nick" => LineKind::Nick {
            new: extra.unwrap_or_default(),
        },
        "topic" => LineKind::Topic,
        "mode" => LineKind::Mode,
        "server" => LineKind::Server,
        "error" => LineKind::Error,
        _ => LineKind::Privmsg,
    }
}

const COLUMNS: &str =
    "id, network, buffer, kind, extra, sender, text, time_ms, msgid, own, highlight";

fn row(r: &rusqlite::Row<'_>) -> Result<Stored> {
    Ok(Stored {
        id: r.get(0)?,
        network: r.get(1)?,
        buffer: r.get(2)?,
        kind: decode_kind(&r.get::<_, String>(3)?, r.get(4)?),
        sender: r.get(5)?,
        text: r.get(6)?,
        time: from_ms(r.get(7)?),
        msgid: r.get(8)?,
        own: r.get(9)?,
        highlight: r.get(10)?,
    })
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    /// Store a line. Returns `None` when a line with the same msgid is already logged,
    /// which is how bouncer and chathistory replays stay deduplicated.
    pub fn insert(&self, network: &str, line: &Line) -> Result<Option<i64>> {
        let buffer = line.buffer.as_deref().unwrap_or(SERVER_BUFFER);
        let (kind, extra) = encode_kind(&line.kind);
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO messages (network, buffer, kind, extra, sender, text, time_ms, msgid, own, highlight)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                network,
                buffer.to_lowercase(),
                kind,
                extra,
                line.sender,
                line.text,
                to_ms(line.time),
                line.msgid,
                line.own,
                line.highlight
            ],
        )?;
        Ok((n == 1).then(|| self.conn.last_insert_rowid()))
    }

    /// Up to `limit` lines older than `before_id` (or the newest), returned oldest first.
    pub fn page(
        &self,
        network: &str,
        buffer: &str,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<Stored>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM messages WHERE network = ?1 AND buffer = ?2 AND id < ?3
             ORDER BY time_ms DESC, id DESC LIMIT ?4"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut v: Vec<Stored> = stmt
            .query_map(
                params![
                    network,
                    buffer.to_lowercase(),
                    before_id.unwrap_or(i64::MAX),
                    limit
                ],
                row,
            )?
            .collect::<Result<_>>()?;
        v.reverse();
        Ok(v)
    }

    /// Full-text search using FTS5 query syntax, newest first.
    pub fn search(
        &self,
        query: &str,
        network: Option<&str>,
        buffer: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Stored>> {
        let cols = COLUMNS
            .split(", ")
            .map(|c| format!("m.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {cols} FROM messages_fts f JOIN messages m ON m.id = f.rowid
             WHERE messages_fts MATCH ?1
               AND (?2 IS NULL OR m.network = ?2)
               AND (?3 IS NULL OR m.buffer = ?3)
             ORDER BY m.time_ms DESC LIMIT ?4"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        stmt.query_map(
            params![
                quote_fts(query),
                network,
                buffer.map(str::to_lowercase),
                limit
            ],
            row,
        )?
        .collect()
    }

    pub fn set_read_marker(&self, network: &str, buffer: &str, time: SystemTime) -> Result<()> {
        self.conn.execute(
            "INSERT INTO read_markers (network, buffer, time_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(network, buffer) DO UPDATE SET time_ms = max(time_ms, excluded.time_ms)",
            params![network, buffer.to_lowercase(), to_ms(time)],
        )?;
        Ok(())
    }

    pub fn read_marker(&self, network: &str, buffer: &str) -> Result<Option<SystemTime>> {
        self.conn
            .query_row(
                "SELECT time_ms FROM read_markers WHERE network = ?1 AND buffer = ?2",
                params![network, buffer.to_lowercase()],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|o| o.map(from_ms))
    }

    pub fn unread_count(&self, network: &str, buffer: &str) -> Result<u64> {
        let since = self.read_marker(network, buffer)?.map(to_ms).unwrap_or(0);
        self.conn.query_row(
            "SELECT count(*) FROM messages WHERE network = ?1 AND buffer = ?2 AND time_ms > ?3
               AND own = 0 AND kind IN ('privmsg', 'notice', 'action')",
            params![network, buffer.to_lowercase(), since],
            |r| r.get::<_, i64>(0).map(|n| n as u64),
        )
    }

    /// Buffers that have logged lines on a network, most recently active first.
    pub fn buffers(&self, network: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT buffer FROM messages WHERE network = ?1 GROUP BY buffer ORDER BY max(time_ms) DESC",
        )?;
        stmt.query_map(params![network], |r| r.get(0))?.collect()
    }
}

/// Treat plain user input as a phrase-per-word query so punctuation cannot
/// trip FTS5 syntax errors; input already using quotes or operators passes through.
fn quote_fts(q: &str) -> String {
    let advanced = q.contains('"')
        || q.contains(" OR ")
        || q.contains(" AND ")
        || q.contains(" NOT ")
        || q.ends_with('*');
    if advanced {
        return q.to_string();
    }
    q.split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(buffer: &str, text: &str, ms: u64, msgid: Option<&str>) -> Line {
        Line {
            network: 1,
            buffer: Some(buffer.into()),
            kind: LineKind::Privmsg,
            sender: Some("bob".into()),
            text: text.into(),
            time: UNIX_EPOCH + Duration::from_millis(ms),
            msgid: msgid.map(str::to_string),
            own: false,
            highlight: false,
            history: false,
            reply_to: None,
        }
    }

    #[test]
    fn insert_page_dedup() {
        let s = Store::in_memory().unwrap();
        for i in 0..10 {
            s.insert("net", &line("#A", &format!("m{i}"), i * 1000, None))
                .unwrap();
        }
        assert!(
            s.insert("net", &line("#a", "x", 50_000, Some("id1")))
                .unwrap()
                .is_some()
        );
        assert!(
            s.insert("net", &line("#a", "x", 50_000, Some("id1")))
                .unwrap()
                .is_none()
        );

        let newest = s.page("net", "#a", None, 4).unwrap();
        assert_eq!(
            newest.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["m7", "m8", "m9", "x"]
        );
        let older = s.page("net", "#a", Some(newest[0].id), 3).unwrap();
        assert_eq!(
            older.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["m4", "m5", "m6"]
        );
    }

    #[test]
    fn kinds_roundtrip() {
        let s = Store::in_memory().unwrap();
        let mut l = line("#a", "", 1, None);
        l.kind = LineKind::Kick {
            target: "eve".into(),
            reason: Some("spam bot".into()),
        };
        s.insert("net", &l).unwrap();
        assert_eq!(s.page("net", "#a", None, 1).unwrap()[0].kind, l.kind);
    }

    #[test]
    fn search_and_markers() {
        let s = Store::in_memory().unwrap();
        s.insert("net", &line("#a", "the build is cursed", 1000, None))
            .unwrap();
        s.insert("net", &line("#b", "builds are fine", 2000, None))
            .unwrap();
        s.insert("other", &line("#a", "cursed again", 3000, None))
            .unwrap();

        assert_eq!(s.search("cursed", None, None, 10).unwrap().len(), 2);
        assert_eq!(s.search("cursed", Some("net"), None, 10).unwrap().len(), 1);
        assert_eq!(s.search("build*", None, None, 10).unwrap().len(), 2);
        // Punctuation in plain input must not raise a syntax error.
        assert!(s.search("is (cursed", None, None, 10).is_ok());

        assert_eq!(s.unread_count("net", "#a").unwrap(), 1);
        s.set_read_marker("net", "#a", UNIX_EPOCH + Duration::from_millis(1000))
            .unwrap();
        assert_eq!(s.unread_count("net", "#a").unwrap(), 0);
        // Markers never move backwards.
        s.set_read_marker("net", "#a", UNIX_EPOCH).unwrap();
        assert_eq!(
            s.read_marker("net", "#a").unwrap(),
            Some(UNIX_EPOCH + Duration::from_millis(1000))
        );
        assert_eq!(s.buffers("net").unwrap(), ["#b", "#a"]);
    }
}
