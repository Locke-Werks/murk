//! Drawing the terminal UI.

use murk_client::App;
use murk_core::{ConnState, Line as IrcLine, LineKind};
use murk_proto::format::{self, Color as IrcColor};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const SIDEBAR: u16 = 20;
const NICKLIST: u16 = 18;

pub struct Theme {
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub highlight: Color,
    pub own: Color,
    pub bar_bg: Color,
    pub nick_palette: &'static [Color],
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The desktop client's palettes, by the same names, so one `theme` setting serves both.
/// Foreground stays the terminal's own so light and dark terminals both read well.
fn theme_named(name: &str) -> Theme {
    let (accent, highlight, own, bar_bg) = match name {
        "aurora" => (0x34d399, 0xfde047, 0xf472b6, 0x10221d),
        "ember" => (0xfb923c, 0xfacc15, 0xf43f5e, 0x211814),
        "synthwave" => (0x22d3ee, 0xfde047, 0xf472b6, 0x1d1238),
        "mocha" => (0xcba6f7, 0xf9e2af, 0xa6e3a1, 0x313244),
        "phosphor" => (0x7dff9b, 0xffd37d, 0xc6ff7d, 0x081208),
        "paper" => (0x0f766e, 0xb45309, 0x7c3aed, 0xe8e3d7),
        _ => (0x5eead4, 0xfbbf24, 0xa78bfa, 0x171b28),
    };
    Theme {
        accent: rgb(accent),
        highlight: rgb(highlight),
        own: rgb(own),
        bar_bg: rgb(bar_bg),
        ..DARK
    }
}

pub const DARK: Theme = Theme {
    fg: Color::Reset,
    dim: Color::DarkGray,
    accent: Color::Rgb(0x8b, 0xa8, 0xd8),
    highlight: Color::Rgb(0xf3, 0x8b, 0xa8),
    own: Color::Rgb(0xa6, 0xe3, 0xa1),
    bar_bg: Color::Rgb(0x31, 0x32, 0x44),
    nick_palette: &[
        Color::Rgb(0xf5, 0xc2, 0xe7),
        Color::Rgb(0xcb, 0xa6, 0xf7),
        Color::Rgb(0xfa, 0xb3, 0x87),
        Color::Rgb(0xf9, 0xe2, 0xaf),
        Color::Rgb(0x94, 0xe2, 0xd5),
        Color::Rgb(0x89, 0xdc, 0xeb),
        Color::Rgb(0x74, 0xc7, 0xec),
        Color::Rgb(0xb4, 0xbe, 0xfe),
        Color::Rgb(0xf2, 0xcd, 0xcd),
        Color::Rgb(0xeb, 0xa0, 0xac),
    ],
};

fn nick_color(theme: &Theme, nick: &str) -> Color {
    // FNV-1a keeps a nick's color stable across sessions.
    let h = nick.to_lowercase().bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    theme.nick_palette[h as usize % theme.nick_palette.len()]
}

fn irc_color(c: IrcColor) -> Option<Color> {
    match c {
        IrcColor::Default => None,
        IrcColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
        IrcColor::Palette(n) => format::palette_rgb(n).map(|(r, g, b)| Color::Rgb(r, g, b)),
    }
}

fn styled_text(text: &str, base: Style) -> Vec<Span<'static>> {
    format::parse(text)
        .into_iter()
        .map(|s| {
            let mut st = base;
            let (mut fg, mut bg) = (irc_color(s.style.fg), irc_color(s.style.bg));
            if s.style.reverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            if let Some(f) = fg {
                st = st.fg(f);
            }
            if let Some(b) = bg {
                st = st.bg(b);
            }
            if s.style.bold {
                st = st.add_modifier(Modifier::BOLD);
            }
            if s.style.italic {
                st = st.add_modifier(Modifier::ITALIC);
            }
            if s.style.underline {
                st = st.add_modifier(Modifier::UNDERLINED);
            }
            if s.style.strikethrough {
                st = st.add_modifier(Modifier::CROSSED_OUT);
            }
            Span::styled(s.text.replace('\t', "    "), st)
        })
        .collect()
}

/// Format with a fixed UTC offset, sampled once at startup.
fn timestamp(t: SystemTime, fmt: &str, offset_secs: i64) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + offset_secs;
    let sod = secs.rem_euclid(86_400);
    fmt.replace("%H", &format!("{:02}", sod / 3600))
        .replace("%M", &format!("{:02}", (sod / 60) % 60))
        .replace("%S", &format!("{:02}", sod % 60))
}

fn render_line(theme: &Theme, l: &IrcLine, ts: String) -> (Vec<Span<'static>>, usize) {
    let mut spans = vec![Span::styled(
        format!("{ts} "),
        Style::default().fg(theme.dim),
    )];
    let sender = l.sender.clone().unwrap_or_default();
    let ncolor = if l.own {
        theme.own
    } else {
        nick_color(theme, &sender)
    };
    let base = if l.highlight {
        Style::default().fg(theme.highlight)
    } else if l.history {
        Style::default().fg(theme.fg).add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(theme.fg)
    };
    let event = Style::default().fg(theme.dim);
    let opt = |r: &Option<String>| r.as_ref().map(|r| format!(" ({r})")).unwrap_or_default();
    match &l.kind {
        LineKind::Privmsg => {
            spans.push(Span::styled(
                format!("<{sender}> "),
                Style::default().fg(ncolor).add_modifier(Modifier::BOLD),
            ));
        }
        LineKind::Notice => {
            spans.push(Span::styled(
                format!("-{sender}- "),
                Style::default().fg(ncolor),
            ));
        }
        LineKind::Action => {
            spans.push(Span::styled(
                format!("* {sender} "),
                Style::default().fg(ncolor),
            ));
        }
        LineKind::Join => {
            spans.push(Span::styled(format!("--> {sender} joined"), event));
            return indent_of(spans);
        }
        LineKind::Part { reason } => {
            spans.push(Span::styled(
                format!("<-- {sender} left{}", opt(reason)),
                event,
            ));
            return indent_of(spans);
        }
        LineKind::Quit { reason } => {
            spans.push(Span::styled(
                format!("<-- {sender} quit{}", opt(reason)),
                event,
            ));
            return indent_of(spans);
        }
        LineKind::Kick { target, reason } => {
            spans.push(Span::styled(
                format!("<-- {target} was kicked by {sender}{}", opt(reason)),
                event,
            ));
            return indent_of(spans);
        }
        LineKind::Nick { new } => {
            spans.push(Span::styled(format!("--- {sender} is now {new}"), event));
            return indent_of(spans);
        }
        LineKind::Topic => {
            spans.push(Span::styled(format!("--- {sender} set the topic: "), event));
        }
        LineKind::Mode => {
            spans.push(Span::styled(format!("--- {sender} sets mode "), event));
        }
        LineKind::Server => spans.push(Span::styled("-- ", event)),
        LineKind::Error => spans.push(Span::styled("!! ", Style::default().fg(Color::Red))),
    }
    let (spans, indent) = indent_of(spans);
    let mut spans = spans;
    spans.extend(styled_text(&l.text, base));
    (spans, indent)
}

fn indent_of(spans: Vec<Span<'static>>) -> (Vec<Span<'static>>, usize) {
    let w = spans.iter().map(|s| s.content.width()).sum();
    (spans, w)
}

/// Wrap spans into rows of `width`, breaking at spaces when possible and
/// indenting continuation rows to `indent` so text lines up after the nick.
fn wrap(spans: Vec<Span<'static>>, width: usize, indent: usize) -> Vec<Line<'static>> {
    let width = width.max(8);
    let indent = if indent * 2 > width { 2 } else { indent };
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut col = 0;
    for span in spans {
        let style = span.style;
        let mut word = String::new();
        let flush_word =
            |word: &mut String, rows: &mut Vec<Vec<Span<'static>>>, col: &mut usize| {
                if word.is_empty() {
                    return;
                }
                let ww = word.width();
                if *col + ww > width && *col > indent {
                    rows.push(vec![Span::raw(" ".repeat(indent))]);
                    *col = indent;
                    if word.starts_with(' ') {
                        let t = word.trim_start().to_string();
                        *word = t;
                    }
                }
                // Hard-break words longer than a row.
                let mut chunk = String::new();
                for c in word.chars() {
                    let cw = c.width().unwrap_or(0);
                    if *col + cw > width {
                        rows.last_mut()
                            .unwrap()
                            .push(Span::styled(std::mem::take(&mut chunk), style));
                        rows.push(vec![Span::raw(" ".repeat(indent))]);
                        *col = indent;
                    }
                    chunk.push(c);
                    *col += cw;
                }
                rows.last_mut().unwrap().push(Span::styled(chunk, style));
                word.clear();
            };
        for c in span.content.chars() {
            if c == ' ' && !word.trim().is_empty() {
                flush_word(&mut word, &mut rows, &mut col);
            }
            word.push(c);
        }
        flush_word(&mut word, &mut rows, &mut col);
    }
    rows.into_iter().map(Line::from).collect()
}

pub fn draw(f: &mut Frame, app: &mut App, tz_offset: i64) {
    let theme = &theme_named(&app.config.ui.theme);
    let area = f.area();
    let [main, status, input] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let show_nicks = app.config.ui.nicklist
        && app.current().is_some_and(|b| b.is_channel_like())
        && main.width > SIDEBAR + NICKLIST + 30;
    let sidebar = if main.width > SIDEBAR + 30 {
        SIDEBAR
    } else {
        0
    };
    let [side, chat, nicks] = Layout::horizontal([
        Constraint::Length(sidebar),
        Constraint::Min(10),
        Constraint::Length(if show_nicks { NICKLIST } else { 0 }),
    ])
    .areas(main);

    if sidebar > 0 {
        draw_buffers(f, app, side, theme);
    }
    draw_chat(f, app, chat, theme, tz_offset);
    if show_nicks {
        draw_nicks(f, app, nicks, theme);
    }
    draw_status(f, app, status, theme);

    let prompt = app
        .current()
        .and_then(|b| app.networks.get(&b.network))
        .map(|n| format!("[{}] ", n.nick))
        .unwrap_or_else(|| "> ".into());
    let pw = prompt.width();
    let avail = (input.width as usize).saturating_sub(pw + 1);
    let cursor = app.editor.cursor_col();
    // Scroll the input horizontally to keep the cursor visible.
    let skip_cols = cursor.saturating_sub(avail);
    let mut shown = String::new();
    let mut col = 0;
    for c in app.editor.text().chars() {
        let w = c.width().unwrap_or(0);
        if col >= skip_cols {
            shown.push(c);
        }
        col += w;
    }
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt, Style::default().fg(theme.accent)),
            Span::raw(shown),
        ])),
        input,
    );
    f.set_cursor_position((input.x + (pw + cursor - skip_cols) as u16, input.y));
}

fn draw_buffers(f: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let items: Vec<ListItem> = app
        .buffers
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let label = match &b.name {
                None => {
                    let n = app.networks.get(&b.network);
                    let name = n.map(|n| n.name.as_str()).unwrap_or("?");
                    let mark = match n.map(|n| n.state) {
                        Some(ConnState::Connected) => "",
                        _ => " (off)",
                    };
                    format!("{name}{mark}")
                }
                Some(n) => format!("  {n}"),
            };
            let num = format!("{:>2} ", i + 1);
            let mut style = Style::default().fg(theme.fg);
            if b.name.is_some() && !b.joined && b.is_channel_like() {
                style = style.fg(theme.dim);
            }
            if b.unread > 0 {
                style = style.fg(theme.accent).add_modifier(Modifier::BOLD);
            }
            if b.highlighted {
                style = style.fg(theme.highlight).add_modifier(Modifier::BOLD);
            }
            if i == app.active {
                style = style.bg(theme.bar_bg).add_modifier(Modifier::BOLD);
            }
            let unread = if b.unread > 0 && i != app.active {
                format!(" {}", b.unread)
            } else {
                String::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(num, Style::default().fg(theme.dim)),
                Span::styled(label, style),
                Span::styled(unread, Style::default().fg(theme.dim)),
            ]))
        })
        .collect();
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(theme.dim)),
        ),
        area,
    );
}

fn draw_chat(f: &mut Frame, app: &mut App, area: Rect, theme: &Theme, tz_offset: i64) {
    let width = area.width.saturating_sub(1) as usize;
    let height = area.height as usize;
    let show_jp = app.config.ui.show_joins_parts;
    let fmt = app.config.ui.timestamp_format.clone();
    let mut want_older = false;
    let Some(buf) = app.buffers.get_mut(app.active) else {
        let hint = Paragraph::new(vec![
            Line::from("Murk"),
            Line::from(""),
            Line::from("/connect <network> or /connect host:port to start, /help for commands."),
        ]);
        f.render_widget(hint, area);
        return;
    };

    // Build rows bottom-up, only as many as needed to fill the view plus the scroll offset.
    let need = height + buf.scroll;
    let mut rows: Vec<Line<'static>> = Vec::new();
    for (i, l) in buf.lines.iter().enumerate().rev() {
        if !show_jp
            && matches!(
                l.kind,
                LineKind::Join | LineKind::Part { .. } | LineKind::Quit { .. }
            )
        {
            continue;
        }
        // Runs of joins and parts show once, as a summary on the last line of the run.
        let (folded, summary) = murk_client::fold::fold_at(&buf.lines, i);
        if folded {
            continue;
        }
        let (mut spans, indent) = match summary {
            Some(s) => (
                vec![
                    Span::styled(
                        format!("{} ", timestamp(l.time, &fmt, tz_offset)),
                        Style::default().fg(theme.dim),
                    ),
                    Span::styled(format!("\u{21c5} {s}"), Style::default().fg(theme.dim)),
                ],
                0,
            ),
            None => render_line(theme, l, timestamp(l.time, &fmt, tz_offset)),
        };
        // Reactions trail the message: " 👍 2  🎉 1".
        if let Some(rs) = l.msgid.as_deref().and_then(|id| buf.reactions.get(id)) {
            for (emoji, who) in rs {
                spans.push(Span::styled(
                    format!("  {emoji} {}", who.len()),
                    Style::default().fg(theme.accent),
                ));
            }
        }
        let mut wrapped = wrap(spans, width, indent);
        wrapped.reverse();
        rows.extend(wrapped);
        // A reply shows what it answers on the row above it.
        if let Some(id) = &l.reply_to {
            let ctx = match buf
                .lines
                .iter()
                .rev()
                .find(|x| x.msgid.as_deref() == Some(id))
            {
                Some(r) => format!(
                    "{}: {}",
                    r.sender.as_deref().unwrap_or(""),
                    format::strip(&r.text)
                ),
                None => "an earlier message".into(),
            };
            let mut row = vec![
                Span::raw(" ".repeat(indent.saturating_sub(2))),
                Span::styled(format!("\u{21b3} {ctx}"), Style::default().fg(theme.dim)),
            ];
            // One row only: trim to the width.
            let mut text: String = row.iter().map(|s| s.content.to_string()).collect();
            if text.width() > width {
                text = text
                    .chars()
                    .take(width.saturating_sub(1))
                    .collect::<String>()
                    + "\u{2026}";
                row = vec![Span::styled(text, Style::default().fg(theme.dim))];
            }
            rows.push(Line::from(row));
        }
        if rows.len() >= need {
            break;
        }
    }
    let max_scroll = rows.len().saturating_sub(height);
    if buf.scroll > max_scroll {
        buf.scroll = max_scroll;
        want_older = true;
    }
    let visible: Vec<Line> = rows
        .into_iter()
        .skip(buf.scroll)
        .take(height)
        .rev()
        .collect();
    let pad = height.saturating_sub(visible.len());
    let mut lines = vec![Line::from(""); pad];
    lines.extend(visible);
    f.render_widget(Paragraph::new(lines), area);

    if want_older {
        app.load_older();
    }
}

fn draw_nicks(f: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let Some(b) = app.current() else { return };
    let Some(name) = &b.name else { return };
    let members = app
        .engine
        .with_state(b.network, |s| s.sorted_members(name))
        .unwrap_or_default();
    let items: Vec<ListItem> = members
        .iter()
        .take(area.height as usize)
        .map(|m| {
            let p = m
                .highest_prefix()
                .map(String::from)
                .unwrap_or_else(|| " ".into());
            let mut st = Style::default().fg(nick_color(theme, &m.nick));
            if m.away {
                st = st.fg(theme.dim);
            }
            ListItem::new(Line::from(vec![
                Span::styled(p, Style::default().fg(theme.accent)),
                Span::styled(m.nick.clone(), st),
            ]))
        })
        .collect();
    let title = format!(" {} users ", members.len());
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::LEFT)
                .title(title)
                .border_style(Style::default().fg(theme.dim)),
        ),
        area,
    );
}

fn draw_status(f: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let bar = Style::default().bg(theme.bar_bg).fg(theme.fg);
    let mut parts: Vec<Span> = Vec::new();
    if let Some(b) = app.current() {
        let net = app.networks.get(&b.network);
        parts.push(Span::styled(
            format!(" {} ", net.map(|n| n.name.as_str()).unwrap_or("")),
            bar.fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
        if let Some(name) = &b.name {
            parts.push(Span::styled(
                format!("{name} "),
                bar.add_modifier(Modifier::BOLD),
            ));
            let topic = app
                .engine
                .with_state(b.network, |s| {
                    s.channel(name).map(|c| (c.topic.clone(), c.modes.clone()))
                })
                .flatten();
            if let Some((topic, modes)) = topic {
                if !modes.is_empty() {
                    parts.push(Span::styled(format!("+{modes} "), bar.fg(theme.dim)));
                }
                if let Some(t) = topic {
                    parts.push(Span::styled(format::strip(&t), bar));
                }
            }
        }
        if b.scroll > 0 {
            parts.push(Span::styled(" [scrolled] ", bar.fg(theme.highlight)));
        }
        if !b.typing.is_empty() {
            let who: Vec<&str> = b.typing.keys().map(String::as_str).collect();
            parts.push(Span::styled(
                format!(" {} typing...", who.join(", ")),
                bar.fg(theme.dim),
            ));
        }
    }
    if let Some((s, _)) = &app.status {
        parts.push(Span::styled(format!("  {s}"), bar.fg(theme.highlight)));
    }
    f.render_widget(Paragraph::new(Line::from(parts)).style(bar), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_with_indent() {
        let spans = vec![Span::raw("<bob> "), Span::raw("one two three four five")];
        let rows = wrap(spans, 16, 6);
        let text: Vec<String> = rows
            .iter()
            .map(|r| r.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(text[0], "<bob> one two");
        assert!(text[1].starts_with("      "));
        assert!(text.iter().all(|t| t.width() <= 16));
    }

    #[test]
    fn hard_breaks_long_words() {
        let rows = wrap(vec![Span::raw("x".repeat(30))], 10, 0);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn timestamps() {
        let t = UNIX_EPOCH + std::time::Duration::from_secs(3600 * 13 + 65);
        assert_eq!(timestamp(t, "%H:%M:%S", 0), "13:01:05");
        assert_eq!(timestamp(t, "%H:%M", -3600 * 5), "08:01");
    }
}
