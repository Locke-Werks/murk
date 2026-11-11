//! mIRC formatting codes to styled spans, plus stripping.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    /// mIRC palette index 0-98.
    Palette(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub monospace: bool,
    pub reverse: bool,
    pub fg: Color,
    pub bg: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

const BOLD: char = '\x02';
const COLOR: char = '\x03';
const HEX_COLOR: char = '\x04';
const RESET: char = '\x0f';
const MONO: char = '\x11';
const REVERSE: char = '\x16';
const ITALIC: char = '\x1d';
const STRIKE: char = '\x1e';
const UNDERLINE: char = '\x1f';

fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<u8> {
    let mut n: Option<u8> = None;
    for _ in 0..2 {
        match chars.peek() {
            Some(c) if c.is_ascii_digit() => {
                let d = *c as u8 - b'0';
                n = Some(n.unwrap_or(0) * 10 + d);
                chars.next();
            }
            _ => break,
        }
    }
    n
}

fn take_hex(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<(u8, u8, u8)> {
    let mut probe = chars.clone();
    let mut s = String::with_capacity(6);
    for _ in 0..6 {
        match probe.next() {
            Some(c) if c.is_ascii_hexdigit() => s.push(c),
            _ => return None,
        }
    }
    *chars = probe;
    let v = u32::from_str_radix(&s, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn palette(n: u8) -> Color {
    if n == 99 {
        Color::Default
    } else {
        Color::Palette(n)
    }
}

pub fn parse(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut style = Style::default();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();

    let flush = |buf: &mut String, spans: &mut Vec<Span>, style: Style| {
        if !buf.is_empty() {
            spans.push(Span {
                text: std::mem::take(buf),
                style,
            });
        }
    };

    while let Some(c) = chars.next() {
        let before = style;
        match c {
            BOLD => style.bold = !style.bold,
            ITALIC => style.italic = !style.italic,
            UNDERLINE => style.underline = !style.underline,
            STRIKE => style.strikethrough = !style.strikethrough,
            MONO => style.monospace = !style.monospace,
            REVERSE => style.reverse = !style.reverse,
            RESET => style = Style::default(),
            COLOR => match take_digits(&mut chars) {
                None => {
                    style.fg = Color::Default;
                    style.bg = Color::Default;
                }
                Some(fg) => {
                    style.fg = palette(fg);
                    if chars.peek() == Some(&',') {
                        let mut probe = chars.clone();
                        probe.next();
                        if let Some(bg) = take_digits(&mut probe) {
                            style.bg = palette(bg);
                            chars = probe;
                        }
                    }
                }
            },
            HEX_COLOR => match take_hex(&mut chars) {
                None => {
                    style.fg = Color::Default;
                    style.bg = Color::Default;
                }
                Some((r, g, b)) => {
                    style.fg = Color::Rgb(r, g, b);
                    if chars.peek() == Some(&',') {
                        let mut probe = chars.clone();
                        probe.next();
                        if let Some((r, g, b)) = take_hex(&mut probe) {
                            style.bg = Color::Rgb(r, g, b);
                            chars = probe;
                        }
                    }
                }
            },
            c => {
                buf.push(c);
                continue;
            }
        }
        if style != before {
            let mut s = std::mem::take(&mut buf);
            if !s.is_empty() {
                spans.push(Span {
                    text: std::mem::take(&mut s),
                    style: before,
                });
            }
        }
    }
    flush(&mut buf, &mut spans, style);
    spans
}

pub fn strip(text: &str) -> String {
    parse(text).into_iter().map(|s| s.text).collect()
}

/// The 99-entry mIRC palette as RGB, for frontends.
pub fn palette_rgb(n: u8) -> Option<(u8, u8, u8)> {
    const BASE: [u32; 16] = [
        0xffffff, 0x000000, 0x00007f, 0x009300, 0xff0000, 0x7f0000, 0x9c009c, 0xfc7f00, 0xffff00,
        0x00fc00, 0x009393, 0x00ffff, 0x0000fc, 0xff00ff, 0x7f7f7f, 0xd2d2d2,
    ];
    const EXT: [u32; 83] = [
        0x470000, 0x472100, 0x474700, 0x324700, 0x004700, 0x00472c, 0x004747, 0x002747, 0x000047,
        0x2e0047, 0x470047, 0x47002a, 0x740000, 0x743a00, 0x747400, 0x517400, 0x007400, 0x007449,
        0x007474, 0x004074, 0x000074, 0x4b0074, 0x740074, 0x740045, 0xb50000, 0xb56300, 0xb5b500,
        0x7db500, 0x00b500, 0x00b571, 0x00b5b5, 0x0063b5, 0x0000b5, 0x7500b5, 0xb500b5, 0xb5006b,
        0xff0000, 0xff8c00, 0xffff00, 0xb2ff00, 0x00ff00, 0x00ffa0, 0x00ffff, 0x008cff, 0x0000ff,
        0xa500ff, 0xff00ff, 0xff0098, 0xff5959, 0xffb459, 0xffff71, 0xcfff60, 0x6fff6f, 0x65ffc9,
        0x6dffff, 0x59b4ff, 0x5959ff, 0xc459ff, 0xff66ff, 0xff59bc, 0xff9c9c, 0xffd39c, 0xffff9c,
        0xe2ff9c, 0x9cff9c, 0x9cffdb, 0x9cffff, 0x9cd3ff, 0x9c9cff, 0xdc9cff, 0xff9cff, 0xff94d3,
        0x000000, 0x131313, 0x282828, 0x363636, 0x4d4d4d, 0x656565, 0x818181, 0x9f9f9f, 0xbcbcbc,
        0xe2e2e2, 0xffffff,
    ];
    let v = match n {
        0..=15 => BASE[n as usize],
        16..=98 => EXT[(n - 16) as usize],
        _ => return None,
    };
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_and_color() {
        let spans = parse("a\x02b\x0304,12c\x0fd");
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0].text, "a");
        assert!(spans[1].style.bold);
        assert_eq!(spans[2].style.fg, Color::Palette(4));
        assert_eq!(spans[2].style.bg, Color::Palette(12));
        assert_eq!(spans[3].style, Style::default());
    }

    #[test]
    fn comma_without_bg_is_text() {
        let spans = parse("\x034,hi");
        assert_eq!(spans[0].text, ",hi");
        assert_eq!(spans[0].style.fg, Color::Palette(4));
    }

    #[test]
    fn hex_color() {
        let spans = parse("\x04ff8000x");
        assert_eq!(spans[0].style.fg, Color::Rgb(0xff, 0x80, 0));
    }

    #[test]
    fn strips() {
        assert_eq!(strip("\x02\x0312hi\x03 \x1dthere\x0f"), "hi there");
    }

    #[test]
    fn palette_bounds() {
        assert_eq!(palette_rgb(4), Some((0xff, 0, 0)));
        assert_eq!(palette_rgb(98), Some((0xff, 0xff, 0xff)));
        assert_eq!(palette_rgb(99), None);
    }
}
