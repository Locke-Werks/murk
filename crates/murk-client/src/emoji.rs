//! Emoji shortcodes (`:tada:`) shared by both clients, plus the picker's catalog.

/// (shortcode, emoji, category). Ordered by category for the picker.
pub const EMOJI: &[(&str, &str, &str)] = &[
    ("smile", "😄", "people"),
    ("grin", "😁", "people"),
    ("joy", "😂", "people"),
    ("rofl", "🤣", "people"),
    ("slight_smile", "🙂", "people"),
    ("wink", "😉", "people"),
    ("blush", "😊", "people"),
    ("heart_eyes", "😍", "people"),
    ("kiss", "😘", "people"),
    ("yum", "😋", "people"),
    ("stuck_out_tongue", "😛", "people"),
    ("thinking", "🤔", "people"),
    ("neutral", "😐", "people"),
    ("expressionless", "😑", "people"),
    ("smirk", "😏", "people"),
    ("unamused", "😒", "people"),
    ("roll_eyes", "🙄", "people"),
    ("grimace", "😬", "people"),
    ("relieved", "😌", "people"),
    ("sleeping", "😴", "people"),
    ("sunglasses", "😎", "people"),
    ("nerd", "🤓", "people"),
    ("confused", "😕", "people"),
    ("worried", "😟", "people"),
    ("open_mouth", "😮", "people"),
    ("astonished", "😲", "people"),
    ("flushed", "😳", "people"),
    ("pleading", "🥺", "people"),
    ("cry", "😢", "people"),
    ("sob", "😭", "people"),
    ("scream", "😱", "people"),
    ("angry", "😠", "people"),
    ("rage", "😡", "people"),
    ("skull", "💀", "people"),
    ("clown", "🤡", "people"),
    ("ghost", "👻", "people"),
    ("alien", "👽", "people"),
    ("robot", "🤖", "people"),
    ("poop", "💩", "people"),
    ("facepalm", "🤦", "people"),
    ("shrug", "🤷", "people"),
    ("wave", "👋", "hands"),
    ("ok_hand", "👌", "hands"),
    ("+1", "👍", "hands"),
    ("thumbsup", "👍", "hands"),
    ("-1", "👎", "hands"),
    ("thumbsdown", "👎", "hands"),
    ("clap", "👏", "hands"),
    ("raised_hands", "🙌", "hands"),
    ("pray", "🙏", "hands"),
    ("muscle", "💪", "hands"),
    ("point_up", "☝️", "hands"),
    ("v", "✌️", "hands"),
    ("crossed_fingers", "🤞", "hands"),
    ("metal", "🤘", "hands"),
    ("eyes", "👀", "hands"),
    ("brain", "🧠", "hands"),
    ("heart", "❤️", "symbols"),
    ("orange_heart", "🧡", "symbols"),
    ("yellow_heart", "💛", "symbols"),
    ("green_heart", "💚", "symbols"),
    ("blue_heart", "💙", "symbols"),
    ("purple_heart", "💜", "symbols"),
    ("black_heart", "🖤", "symbols"),
    ("broken_heart", "💔", "symbols"),
    ("sparkles", "✨", "symbols"),
    ("star", "⭐", "symbols"),
    ("fire", "🔥", "symbols"),
    ("zap", "⚡", "symbols"),
    ("boom", "💥", "symbols"),
    ("100", "💯", "symbols"),
    ("check", "✅", "symbols"),
    ("x", "❌", "symbols"),
    ("warning", "⚠️", "symbols"),
    ("question", "❓", "symbols"),
    ("exclamation", "❗", "symbols"),
    ("zzz", "💤", "symbols"),
    ("tada", "🎉", "objects"),
    ("confetti", "🎊", "objects"),
    ("gift", "🎁", "objects"),
    ("trophy", "🏆", "objects"),
    ("rocket", "🚀", "objects"),
    ("bulb", "💡", "objects"),
    ("bug", "🐛", "objects"),
    ("wrench", "🔧", "objects"),
    ("hammer", "🔨", "objects"),
    ("lock", "🔒", "objects"),
    ("key", "🔑", "objects"),
    ("computer", "💻", "objects"),
    ("keyboard", "⌨️", "objects"),
    ("floppy", "💾", "objects"),
    ("package", "📦", "objects"),
    ("memo", "📝", "objects"),
    ("pushpin", "📌", "objects"),
    ("link", "🔗", "objects"),
    ("bell", "🔔", "objects"),
    ("hourglass", "⏳", "objects"),
    ("coffee", "☕", "food"),
    ("tea", "🍵", "food"),
    ("beer", "🍺", "food"),
    ("beers", "🍻", "food"),
    ("wine", "🍷", "food"),
    ("pizza", "🍕", "food"),
    ("taco", "🌮", "food"),
    ("cake", "🍰", "food"),
    ("cookie", "🍪", "food"),
    ("popcorn", "🍿", "food"),
    ("cat", "🐱", "nature"),
    ("dog", "🐶", "nature"),
    ("fox", "🦊", "nature"),
    ("penguin", "🐧", "nature"),
    ("crab", "🦀", "nature"),
    ("snake", "🐍", "nature"),
    ("owl", "🦉", "nature"),
    ("unicorn", "🦄", "nature"),
    ("rainbow", "🌈", "nature"),
    ("sun", "☀️", "nature"),
    ("moon", "🌙", "nature"),
    ("cloud", "☁️", "nature"),
    ("fog", "🌫️", "nature"),
    ("snowflake", "❄️", "nature"),
    ("seedling", "🌱", "nature"),
    ("rose", "🌹", "nature"),
];

/// Reactions offered one click away.
pub const QUICK_REACTIONS: &[&str] = &["👍", "❤️", "😂", "🎉", "👀", "🔥"];

pub fn lookup(code: &str) -> Option<&'static str> {
    EMOJI
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, e, _)| *e)
}

/// Replace `:shortcode:` with emoji, leaving unknown codes and URLs (`http://x:80:`) alone.
pub fn expand(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end =
            after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-'));
        match end {
            Some(e) if e > 0 && after[e..].starts_with(':') => {
                if let Some(emoji) = lookup(&after[..e]) {
                    out.push_str(emoji);
                    rest = &after[e + 1..];
                    continue;
                }
                out.push(':');
                rest = after;
            }
            _ => {
                out.push(':');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Shortcodes starting with `prefix`, for completion after a colon.
pub fn complete(prefix: &str) -> Vec<String> {
    EMOJI
        .iter()
        .filter(|(c, _, _)| c.starts_with(prefix))
        .map(|(c, _, _)| format!(":{c}:"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_codes_only() {
        assert_eq!(expand("ship it :rocket: :nope: ok"), "ship it 🚀 :nope: ok");
        assert_eq!(expand(":+1::tada:"), "👍🎉");
        assert_eq!(expand("12:30:45"), "12:30:45");
        assert_eq!(expand("http://host:8080/x"), "http://host:8080/x");
    }
}
