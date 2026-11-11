//! Buffer lines rendered as Qt rich text (the HTML 4 subset QML `Text` understands).

use murk_proto::format::{self, Color};

pub const FG: &str = "#cdd6f4";
pub const OWN: &str = "#89b4fa";

const NICK_PALETTE: [&str; 12] = [
    "#f5e0dc", "#f2cdcd", "#f5c2e7", "#cba6f7", "#f38ba8", "#eba0ac", "#fab387", "#a6e3a1",
    "#94e2d5", "#89dceb", "#74c7ec", "#b4befe",
];

/// Stable per-nick color, FNV-1a so it survives restarts.
pub fn nick_color(nick: &str) -> &'static str {
    let h = nick.to_lowercase().bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    NICK_PALETTE[h as usize % NICK_PALETTE.len()]
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn css(c: Color) -> Option<String> {
    let (r, g, b) = match c {
        Color::Default => return None,
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Palette(n) => format::palette_rgb(n)?,
    };
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// Escape and linkify plain text. URLs become anchors the view opens externally.
fn linkify(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = ["https://", "http://"]
        .iter()
        .filter_map(|p| rest.find(p))
        .min()
    {
        out.push_str(&escape(&rest[..start]));
        let tail = &rest[start..];
        let mut end = tail
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
            .unwrap_or(tail.len());
        // Trailing punctuation usually belongs to the sentence, not the URL.
        while end > 0 && tail[..end].ends_with(['.', ',', ')', ';', ':', '!', '?', '\'']) {
            end -= 1;
        }
        let url = &tail[..end];
        let e = escape(url);
        out.push_str(&format!("<a href=\"{e}\" style=\"color:{OWN}\">{e}</a>"));
        rest = &tail[end..];
    }
    out.push_str(&escape(rest));
    out
}

/// mIRC-formatted text as rich text.
pub fn styled(text: &str) -> String {
    let mut out = String::new();
    for s in format::parse(text) {
        let st = s.style;
        let (mut fg, mut bg) = (css(st.fg), css(st.bg));
        if st.reverse {
            std::mem::swap(&mut fg, &mut bg);
            fg.get_or_insert_with(|| "#1e1e2e".into());
            bg.get_or_insert_with(|| FG.into());
        }
        let mut style = String::new();
        if let Some(f) = fg {
            style.push_str(&format!("color:{f};"));
        }
        if let Some(b) = bg {
            style.push_str(&format!("background-color:{b};"));
        }
        if st.bold {
            style.push_str("font-weight:bold;");
        }
        if st.italic {
            style.push_str("font-style:italic;");
        }
        match (st.underline, st.strikethrough) {
            (true, true) => style.push_str("text-decoration:underline line-through;"),
            (true, false) => style.push_str("text-decoration:underline;"),
            (false, true) => style.push_str("text-decoration:line-through;"),
            _ => {}
        }
        if st.monospace {
            style.push_str("font-family:monospace;");
        }
        let body = linkify(&s.text.replace('\t', "    "));
        if style.is_empty() {
            out.push_str(&body);
        } else {
            out.push_str(&format!("<span style=\"{style}\">{body}</span>"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_links() {
        let s = linkify("see <https://example.com/a?b=1&c=2>, ok");
        assert_eq!(
            s,
            "see &lt;<a href=\"https://example.com/a?b=1&amp;c=2\" style=\"color:#89b4fa\">https://example.com/a?b=1&amp;c=2</a>&gt;, ok"
        );
    }

    #[test]
    fn trailing_punctuation_stays_out() {
        assert!(linkify("go to http://x.org.").ends_with("</a>."));
    }

    #[test]
    fn colors_become_spans() {
        let s = styled("\x0304red\x03 plain");
        assert!(s.starts_with("<span style=\"color:#"));
        assert!(s.ends_with(" plain"));
    }
}
