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

/// The view object for `b.lines[i]`.
pub fn line(b: &Buffer, i: usize, me: &str) -> Value {
    let l = &b.lines[i];
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
    let unread = match (b.read_marker, prev) {
        (Some(m), Some(p)) => p.time <= m && l.time > m && !l.own,
        _ => false,
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
    json!({
        "kind": kind_name(&l.kind),
        "sender": sender,
        "initial": initial(&sender),
        "color": if l.own { html::OWN.to_string() } else { html::nick_color(&sender).to_string() },
        "body": if chat || matches!(l.kind, LineKind::Server | LineKind::Error) { html::styled(&l.text) } else { String::new() },
        "plain": murk_proto::format::strip(&l.text),
        "event": event_text(l),
        "time": t.format("%H:%M").to_string(),
        "stamp": t.format("%A, %B %-d %Y, %H:%M:%S").to_string(),
        "day": day,
        "newDay": new_day,
        "grouped": grouped,
        "unread": unread,
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

    #[test]
    fn initials_skip_decoration() {
        assert_eq!(initial("_[bob]_"), "B");
        assert_eq!(initial("élan"), "É");
        assert_eq!(initial("__"), "?");
    }
}
