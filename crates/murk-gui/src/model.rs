//! Buffer lines as plain objects for QML delegates: grouping, day breaks, the
//! unread divider, replies and reactions are decided here so QML only draws.

use crate::html;
use murk_client::Buffer;
use murk_core::{Line, LineKind};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Consecutive messages from one sender within this window share a header.
const GROUP_WINDOW: Duration = Duration::from_secs(5 * 60);

fn local(t: SystemTime) -> chrono::DateTime<chrono::Local> {
    let ms = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0) as i64;
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .with_timezone(&chrono::Local)
}

fn day_label(t: SystemTime) -> String {
    let d = local(t).date_naive();
    let today = chrono::Local::now().date_naive();
    if d == today {
        "Today".into()
    } else if Some(d) == today.pred_opt() {
        "Yesterday".into()
    } else {
        d.format("%A, %B %-d, %Y").to_string()
    }
}

/// First letter-like character of a nick, skipping decoration like `_` and `[`.
pub fn initial(nick: &str) -> String {
    nick.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

fn is_chat(k: &LineKind) -> bool {
    matches!(k, LineKind::Privmsg | LineKind::Action | LineKind::Notice)
}

fn kind_name(k: &LineKind) -> &'static str {
    match k {
        LineKind::Privmsg => "msg",
        LineKind::Action => "action",
        LineKind::Notice => "notice",
        LineKind::Join => "join",
        LineKind::Part { .. } | LineKind::Quit { .. } | LineKind::Kick { .. } => "leave",
        LineKind::Nick { .. } => "nick",
        LineKind::Topic => "topic",
        LineKind::Mode => "mode",
        LineKind::Server => "server",
        LineKind::Error => "error",
    }
}

/// Text for event lines, which carry no message body of their own.
fn event_text(l: &Line) -> String {
    let who = l.sender.as_deref().unwrap_or("");
    let opt = |r: &Option<String>| {
        r.as_ref()
            .filter(|r| !r.is_empty())
            .map(|r| format!(" ({})", murk_proto::format::strip(r)))
            .unwrap_or_default()
    };
    match &l.kind {
        LineKind::Join => format!("{who} joined"),
        LineKind::Part { reason } => format!("{who} left{}", opt(reason)),
        LineKind::Quit { reason } => format!("{who} quit{}", opt(reason)),
        LineKind::Kick { target, reason } => format!("{target} was kicked by {who}{}", opt(reason)),
        LineKind::Nick { new } => format!("{who} is now known as {new}"),
        LineKind::Topic => format!("{who} changed the topic"),
        LineKind::Mode => format!("{who} set mode {}", l.text),
        _ => String::new(),
    }
}

/// Joins, parts, quits and nick changes: the lines that get folded together.
pub fn is_membership(k: &LineKind) -> bool {
    matches!(
        k,
        LineKind::Join | LineKind::Part { .. } | LineKind::Quit { .. } | LineKind::Nick { .. }
    )
}

/// Membership changes plus the mode lines servers emit around them, which fold too.
pub fn is_foldable(l: &Line) -> bool {
    is_membership(&l.kind)
        || (matches!(l.kind, LineKind::Mode)
            && l.sender
                .as_deref()
                .is_some_and(|s| s.contains('.') && !s.contains('!')))
}

/// One sentence for a run of membership lines, per person, in order of first appearance.
fn fold_summary(run: &[&Line]) -> String {
    #[derive(Default)]
    struct Who {
        acts: Vec<char>,
        renamed: Option<String>,
    }
    let mut order: Vec<String> = Vec::new();
    let mut people: std::collections::HashMap<String, Who> = Default::default();
    for l in run {
        let nick = l.sender.clone().unwrap_or_default();
        let key = nick.to_lowercase();
        if !people.contains_key(&key) {
            order.push(nick.clone());
        }
        let w = people.entry(key).or_default();
        match &l.kind {
            LineKind::Join => w.acts.push('j'),
            LineKind::Part { .. } | LineKind::Quit { .. } => w.acts.push('l'),
            LineKind::Nick { new } => w.renamed = Some(new.clone()),
            _ => {}
        }
    }
    let describe = |w: &Who| -> String {
        let a = &w.acts;
        let mut s = match (a.first(), a.last(), a.len()) {
            (None, _, _) => String::new(),
            (_, _, 1) if a[0] == 'j' => "joined".into(),
            (_, _, 1) => "left".into(),
            (Some('l'), Some('j'), 2) => "reconnected".into(),
            (Some('l'), Some('j'), n) => format!("reconnected {} times", n / 2),
            (Some('j'), Some('l'), 2) => "joined and left".into(),
            (_, Some('j'), n) => format!("joined and left {} times, then came back", n / 2),
            (_, _, n) => format!("joined and left {} times", n.div_ceil(2)),
        };
        if let Some(new) = &w.renamed {
            if !s.is_empty() {
                s.push_str(", ");
            }
            s.push_str(&format!("now known as {new}"));
        }
        s
    };
    if order.len() > 6 {
        let (mut joined, mut left) = (0, 0);
        for w in people.values() {
            match w.acts.last() {
                Some('j') => joined += 1,
                Some('l') => left += 1,
                _ => {}
            }
        }
        return format!("{joined} joined, {left} left, {} changes in all", run.len());
    }
    order
        .iter()
        .map(|n| format!("{n} {}", describe(&people[&n.to_lowercase()])))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The view object for `b.lines[i]`.
pub fn line(b: &Buffer, i: usize, me: &str) -> Value {
    let l = &b.lines[i];
    // A run of membership lines shows once, as a summary on its last line.
    let foldable = is_foldable(l);
    let folded = foldable && b.lines.get(i + 1).is_some_and(is_foldable);
    let mut summary = None;
    if foldable && !folded {
        let mut start = i;
        while start > 0 && i - start < 500 && is_foldable(&b.lines[start - 1]) {
            start -= 1;
        }
        let run: Vec<&Line> = b
            .lines
            .range(start..=i)
            .filter(|l| is_membership(&l.kind))
            .collect();
        if start < i && !run.is_empty() {
            summary = Some(fold_summary(&run));
        }
    }
    let prev = i.checked_sub(1).and_then(|p| b.lines.get(p));
    let t = local(l.time);
    let day = day_label(l.time);
    let new_day = prev.is_none_or(|p| local(p.time).date_naive() != t.date_naive());
    let grouped = !new_day
        && prev.is_some_and(|p| {
            is_chat(&p.kind)
                && matches!(l.kind, LineKind::Privmsg | LineKind::Action)
                && matches!(p.kind, LineKind::Privmsg | LineKind::Action)
                && p.sender == l.sender
                && l.time.duration_since(p.time).unwrap_or_default() < GROUP_WINDOW
                && l.reply_to.is_none()
        });
    // The divider goes on the first message from someone else after the read marker;
    // local notes and joins after the marker do not count.
    let missed = |x: &Line, m: SystemTime| {
        x.time > m
            && !x.own
            && matches!(
                x.kind,
                LineKind::Privmsg | LineKind::Action | LineKind::Notice
            )
    };
    let unread = match b.read_marker {
        Some(m) if i > 0 && missed(l, m) => !b
            .lines
            .range(..i)
            .rev()
            .take_while(|x| x.time > m)
            .any(|x| missed(x, m)),
        _ => false,
    };
    // What you missed, summarised on the unread divider.
    let digest = if unread {
        let mut count = 0;
        let mut mentions = 0;
        let mut who: Vec<(String, usize)> = Vec::new();
        for x in b.lines.range(i..) {
            if !matches!(x.kind, LineKind::Privmsg | LineKind::Action) || x.own {
                continue;
            }
            count += 1;
            mentions += x.highlight as usize;
            let s = x.sender.clone().unwrap_or_default();
            match who.iter_mut().find(|(n, _)| *n == s) {
                Some((_, c)) => *c += 1,
                None => who.push((s, 1)),
            }
        }
        who.sort_by_key(|w| std::cmp::Reverse(w.1));
        let names: Vec<&str> = who.iter().take(2).map(|(n, _)| n.as_str()).collect();
        let others = who.len().saturating_sub(2);
        let from = match (names.len(), others) {
            (0, _) => String::new(),
            (1, _) => format!(" from {}", names[0]),
            (_, 0) => format!(" from {} and {}", names[0], names[1]),
            (_, n) => format!(
                " from {}, {} and {n} other{}",
                names[0],
                names[1],
                if n == 1 { "" } else { "s" }
            ),
        };
        let mut s = format!(
            "{count} new message{}{from}",
            if count == 1 { "" } else { "s" }
        );
        if mentions > 0 {
            s.push_str(&format!(
                "  \u{b7}  {mentions} mention{} you",
                if mentions == 1 { "" } else { "s" }
            ));
        }
        s
    } else {
        String::new()
    };
    let sender = l.sender.clone().unwrap_or_default();
    let reply = l.reply_to.as_deref().map(|id| match b.find_msgid(id) {
        Some(r) => json!({
            "sender": r.sender.clone().unwrap_or_default(),
            "color": html::nick_color(r.sender.as_deref().unwrap_or("")),
            "text": truncate(&murk_proto::format::strip(&r.text), 140),
        }),
        None => json!({ "sender": "", "color": "", "text": "an earlier message" }),
    });
    let reactions: Vec<Value> = l
        .msgid
        .as_deref()
        .and_then(|id| b.reactions.get(id))
        .map(|rs| {
            rs.iter()
                .map(|(e, who)| {
                    json!({
                        "emoji": e,
                        "count": who.len(),
                        "mine": who.iter().any(|w| w.eq_ignore_ascii_case(me)),
                        "who": who.join(", "),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let chat = is_chat(&l.kind) || matches!(l.kind, LineKind::Topic);
    // The network buffer reads as a log: server notices stay one compact line each.
    let server_log = b.name.is_none() && is_chat(&l.kind);
    let body = if server_log && !sender.is_empty() {
        format!(
            "<b>{}</b>&nbsp; {}",
            html::escape(&sender),
            html::styled(&l.text)
        )
    } else if chat || matches!(l.kind, LineKind::Server | LineKind::Error) {
        html::styled(&l.text)
    } else {
        String::new()
    };
    json!({
        "kind": if server_log { "server" } else { kind_name(&l.kind) },
        "sender": sender,
        "initial": initial(&sender),
        "color": if l.own { html::OWN.to_string() } else { html::nick_color(&sender).to_string() },
        "body": body,
        "plain": murk_proto::format::strip(&l.text),
        "event": summary.clone().unwrap_or_else(|| event_text(l)),
        "folded": folded,
        "summary": summary.is_some(),
        "time": t.format("%H:%M").to_string(),
        "stamp": t.format("%A, %B %-d %Y, %H:%M:%S").to_string(),
        "day": day,
        "newDay": new_day,
        "grouped": grouped,
        "unread": unread,
        "digest": digest,
        "own": l.own,
        "highlight": l.highlight,
        "history": l.history,
        "msgid": l.msgid.clone().unwrap_or_default(),
        "reply": reply.unwrap_or(Value::Null),
        "reactions": reactions,
    })
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n).collect();
    t.push('\u{2026}');
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(nick: &str, kind: LineKind) -> Line {
        Line {
            network: 1,
            buffer: Some("#c".into()),
            kind,
            sender: Some(nick.into()),
            text: String::new(),
            time: SystemTime::UNIX_EPOCH,
            msgid: None,
            own: false,
            highlight: false,
            history: false,
            reply_to: None,
        }
    }

    #[test]
    fn folds_membership_runs() {
        let quit = || LineKind::Quit { reason: None };
        let lines = [
            ev("alice", quit()),
            ev("alice", LineKind::Join),
            ev("bob", LineKind::Join),
            ev("carol", LineKind::Join),
            ev("carol", LineKind::Part { reason: None }),
            ev(
                "dave",
                LineKind::Nick {
                    new: "dave_".into(),
                },
            ),
        ];
        let run: Vec<&Line> = lines.iter().collect();
        assert_eq!(
            fold_summary(&run),
            "alice reconnected, bob joined, carol joined and left, dave now known as dave_"
        );
    }

    #[test]
    fn initials_skip_decoration() {
        assert_eq!(initial("_[bob]_"), "B");
        assert_eq!(initial("élan"), "É");
        assert_eq!(initial("__"), "?");
    }
}
