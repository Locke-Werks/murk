//! IRC message parsing and serialization (RFC 1459 framing plus IRCv3 message-tags).

use std::fmt::{self, Write};

/// Server limit for the non-tag part of a line, excluding CRLF.
pub const MAX_BODY_LEN: usize = 510;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub key: String,
    /// `None` for a bare key; `Some("")` is normalized to `None` per spec.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prefix {
    Server(String),
    User {
        nick: String,
        user: Option<String>,
        host: Option<String>,
    },
}

impl Prefix {
    pub fn parse(s: &str) -> Prefix {
        let (nick_user, host) = match s.split_once('@') {
            Some((a, h)) => (a, Some(h.to_string())),
            None => (s, None),
        };
        let (nick, user) = match nick_user.split_once('!') {
            Some((n, u)) => (n, Some(u.to_string())),
            None => (nick_user, None),
        };
        // A bare token containing a dot and no user/host is a server name.
        if user.is_none() && host.is_none() && nick.contains('.') {
            return Prefix::Server(nick.to_string());
        }
        Prefix::User {
            nick: nick.to_string(),
            user,
            host,
        }
    }

    pub fn nick(&self) -> &str {
        match self {
            Prefix::Server(s) => s,
            Prefix::User { nick, .. } => nick,
        }
    }
}

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Prefix::Server(s) => f.write_str(s),
            Prefix::User { nick, user, host } => {
                f.write_str(nick)?;
                if let Some(u) = user {
                    write!(f, "!{u}")?;
                }
                if let Some(h) = host {
                    write!(f, "@{h}")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub tags: Vec<Tag>,
    pub prefix: Option<Prefix>,
    /// Uppercased command or three-digit numeric.
    pub command: String,
    pub params: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    MissingCommand,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => f.write_str("empty line"),
            ParseError::MissingCommand => f.write_str("missing command"),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn unescape_tag_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(':') => out.push(';'),
            Some('s') => out.push(' '),
            Some('\\') => out.push('\\'),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            // A trailing lone backslash is dropped per spec.
            None => {}
        }
    }
    out
}

pub fn escape_tag_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ';' => out.push_str("\\:"),
            ' ' => out.push_str("\\s"),
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

impl Message {
    pub fn new(command: impl Into<String>, params: Vec<String>) -> Self {
        Message {
            tags: Vec::new(),
            prefix: None,
            command: command.into(),
            params,
        }
    }

    pub fn parse(line: &str) -> Result<Message, ParseError> {
        let mut rest = line.trim_end_matches(['\r', '\n']).trim_start_matches(' ');
        if rest.is_empty() {
            return Err(ParseError::Empty);
        }

        let mut tags = Vec::new();
        if let Some(stripped) = rest.strip_prefix('@') {
            let (raw, after) = stripped.split_once(' ').unwrap_or((stripped, ""));
            for item in raw.split(';').filter(|s| !s.is_empty()) {
                let (key, value) = match item.split_once('=') {
                    Some((k, v)) => (k, Some(unescape_tag_value(v)).filter(|v| !v.is_empty())),
                    None => (item, None),
                };
                // Later duplicates win, so drop any earlier occurrence.
                tags.retain(|t: &Tag| t.key != key);
                tags.push(Tag {
                    key: key.to_string(),
                    value,
                });
            }
            rest = after.trim_start_matches(' ');
        }

        let mut prefix = None;
        if let Some(stripped) = rest.strip_prefix(':') {
            let (raw, after) = stripped.split_once(' ').unwrap_or((stripped, ""));
            prefix = Some(Prefix::parse(raw));
            rest = after.trim_start_matches(' ');
        }

        let (command, mut rest) = rest.split_once(' ').unwrap_or((rest, ""));
        if command.is_empty() {
            return Err(ParseError::MissingCommand);
        }

        let mut params = Vec::new();
        loop {
            rest = rest.trim_start_matches(' ');
            if rest.is_empty() {
                break;
            }
            if let Some(trailing) = rest.strip_prefix(':') {
                params.push(trailing.to_string());
                break;
            }
            match rest.split_once(' ') {
                Some((p, after)) => {
                    params.push(p.to_string());
                    rest = after;
                }
                None => {
                    params.push(rest.to_string());
                    break;
                }
            }
        }

        Ok(Message {
            tags,
            prefix,
            command: command.to_ascii_uppercase(),
            params,
        })
    }

    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.key == key)
            .map(|t| t.value.as_deref().unwrap_or(""))
    }

    pub fn has_tag(&self, key: &str) -> bool {
        self.tags.iter().any(|t| t.key == key)
    }

    pub fn with_tag(mut self, key: impl Into<String>, value: Option<String>) -> Self {
        let key = key.into();
        self.tags.retain(|t| t.key != key);
        self.tags.push(Tag { key, value });
        self
    }

    pub fn param(&self, i: usize) -> Option<&str> {
        self.params.get(i).map(String::as_str)
    }

    pub fn source_nick(&self) -> Option<&str> {
        self.prefix.as_ref().map(Prefix::nick)
    }

    /// Serialize without CRLF. Parameters containing spaces, starting with ':'
    /// or being empty are only valid in the last position and get a ':' prefix there.
    pub fn to_wire(&self) -> String {
        let mut out = String::new();
        if !self.tags.is_empty() {
            out.push('@');
            for (i, t) in self.tags.iter().enumerate() {
                if i > 0 {
                    out.push(';');
                }
                out.push_str(&t.key);
                if let Some(v) = &t.value {
                    out.push('=');
                    out.push_str(&escape_tag_value(v));
                }
            }
            out.push(' ');
        }
        if let Some(p) = &self.prefix {
            let _ = write!(out, ":{p} ");
        }
        out.push_str(&self.command);
        let last = self.params.len().saturating_sub(1);
        for (i, p) in self.params.iter().enumerate() {
            out.push(' ');
            if i == last && (p.is_empty() || p.contains(' ') || p.starts_with(':')) {
                out.push(':');
            }
            out.push_str(p);
        }
        out
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_wire())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_line() {
        let m = Message::parse(
            "@time=2026-01-01T00:00:00.000Z;msgid=abc :nick!user@host PRIVMSG #chan :hello world\r\n",
        )
        .unwrap();
        assert_eq!(m.tag("time"), Some("2026-01-01T00:00:00.000Z"));
        assert_eq!(m.tag("msgid"), Some("abc"));
        assert_eq!(m.source_nick(), Some("nick"));
        assert_eq!(m.command, "PRIVMSG");
        assert_eq!(m.params, vec!["#chan", "hello world"]);
    }

    #[test]
    fn tag_escaping_roundtrips() {
        let raw = "a;b c\\d\r\ne";
        let esc = escape_tag_value(raw);
        assert_eq!(esc, "a\\:b\\sc\\\\d\\r\\ne");
        assert_eq!(unescape_tag_value(&esc), raw);
        assert_eq!(unescape_tag_value("x\\"), "x");
        assert_eq!(unescape_tag_value("\\b"), "b");
    }

    #[test]
    fn empty_tag_value_is_bare() {
        let m = Message::parse("@a=;b PING x").unwrap();
        assert_eq!(m.tags[0].value, None);
        assert!(m.has_tag("b"));
    }

    #[test]
    fn duplicate_tags_last_wins() {
        let m = Message::parse("@a=1;a=2 PING x").unwrap();
        assert_eq!(m.tags.len(), 1);
        assert_eq!(m.tag("a"), Some("2"));
    }

    #[test]
    fn server_prefix() {
        let m = Message::parse(":irc.example.net 001 me :Welcome").unwrap();
        assert_eq!(m.prefix, Some(Prefix::Server("irc.example.net".into())));
    }

    #[test]
    fn extra_spaces_and_empty_trailing() {
        let m = Message::parse("  TOPIC   #c   :").unwrap();
        assert_eq!(m.params, vec!["#c", ""]);
        assert_eq!(m.to_wire(), "TOPIC #c :");
    }

    #[test]
    fn serializes_with_tags_and_trailing() {
        let m = Message::new("PRIVMSG", vec!["#c".into(), ":) hi".into()])
            .with_tag("+draft/reply", Some("id 1".into()));
        assert_eq!(m.to_wire(), "@+draft/reply=id\\s1 PRIVMSG #c ::) hi");
        assert_eq!(Message::parse(&m.to_wire()).unwrap(), m);
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(Message::parse("\r\n"), Err(ParseError::Empty));
        assert_eq!(Message::parse("@a=b "), Err(ParseError::MissingCommand));
    }
}
