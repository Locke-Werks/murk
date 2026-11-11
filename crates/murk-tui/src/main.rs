//! `murk`: the terminal IRC client.

mod media;
mod ui;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as TermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use futures::StreamExt;
use murk_client::App;
use murk_config::Config;
use murk_core::Engine;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

const USAGE: &str = "Usage: murk [--config <path>] [--no-log] [network...]
       murk secret set <name>   (stores stdin in the OS keyring)

Starts the terminal client. Networks named on the command line connect at
startup, in addition to those marked autoconnect in the config.
Default config: ";

struct Args {
    config: PathBuf,
    log: bool,
    connect: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: murk_config::default_path(),
        log: true,
        connect: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-c" | "--config" => args.config = it.next().ok_or("--config needs a path")?.into(),
            "--no-log" => args.log = false,
            "-h" | "--help" => {
                return Err(format!("{USAGE}{}", murk_config::default_path().display()));
            }
            "-V" | "--version" => return Err(format!("murk {}", env!("CARGO_PKG_VERSION"))),
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n\n{USAGE}")),
            s => args.connect.push(s.to_string()),
        }
    }
    Ok(args)
}

fn secret_command(args: &[String]) -> i32 {
    let (Some("set"), Some(name), None) =
        (args.first().map(String::as_str), args.get(1), args.get(2))
    else {
        eprintln!("Usage: murk secret set <name>   (the value is read from stdin)");
        return 2;
    };
    let mut value = String::new();
    if std::io::stdin().read_line(&mut value).is_err() || value.trim().is_empty() {
        eprintln!("murk: no value on stdin");
        return 1;
    }
    match murk_config::Secret::store(name, value.trim()) {
        Ok(_) => {
            eprintln!("Stored. Refer to it in the config as {{ keyring = \"{name}\" }}");
            0
        }
        Err(e) => {
            eprintln!("murk: could not store the secret: {e}");
            1
        }
    }
}

#[tokio::main]
async fn main() {
    // `murk secret set <name>` stores stdin in the OS keyring, for config entries
    // written as `{ keyring = "<name>" }`. Reading stdin keeps the value out of
    // shell history and process listings.
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("secret") {
        std::process::exit(secret_command(&raw[1..]));
    }
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            println!("{msg}");
            return;
        }
    };
    let config = match Config::load(&args.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("murk: {e}");
            std::process::exit(1);
        }
    };
    if !args.config.exists() {
        // First run: write the starter config so there is something to edit.
        let _ = config.save(&args.config);
    }
    let store = if args.log {
        let dir = murk_config::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        match murk_store::Store::open(dir.join("logs.db")) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("murk: logging disabled: {e}");
                None
            }
        }
    } else {
        None
    };

    let tz_offset = i64::from(chrono::Local::now().offset().local_minus_utc());
    let engine = Engine::new();
    let mut app = App::new(engine.clone(), store, config);
    app.config_path = Some(args.config.clone());

    let mut startup: Vec<String> = app
        .config
        .networks
        .iter()
        .filter(|n| n.autoconnect)
        .map(|n| n.name.clone())
        .collect();
    startup.extend(args.connect);
    for name in startup {
        if let Err(e) = app.connect(&name) {
            app.set_status(e);
        }
    }
    if app.buffers.is_empty() {
        app.set_status(format!("Config: {}", args.config.display()));
    }

    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    let result = run(&mut terminal, &mut app, engine, tz_offset).await;
    let _ = execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
    ratatui::restore();
    if let Err(e) = result {
        eprintln!("murk: {e}");
    }
}

async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    engine: Engine,
    tz_offset: i64,
) -> std::io::Result<()> {
    // Ask the terminal what it can draw before anything else starts reading stdin.
    let mut media = media::Media::detect();
    if app.config.ui.inline_media {
        app.set_status(format!("Inline media: {}", media.protocol_name()));
    }
    let mut events = engine.subscribe();
    let mut input = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut last_drawn: Vec<(String, ratatui::layout::Rect)> = Vec::new();
    let mut hits = ui::Hits::default();

    loop {
        // Synchronized output: the terminal shows each frame whole, instead of the
        // half-drawn state in between. Without it a picture being replaced (video
        // above all) flickers through its cleared area. Terminals that do not know
        // the mode ignore it.
        let _ = execute!(std::io::stdout(), BeginSynchronizedUpdate);
        terminal.draw(|f| ui::draw(f, app, &mut media, &mut hits, tz_offset))?;
        // Graphics are not cells: a picture that moved or vanished leaves its old
        // pixels behind until something paints over them. Repaint everything then.
        if media.drawn != last_drawn {
            let moved = last_drawn.iter().any(|old| !media.drawn.contains(old));
            last_drawn = media.drawn.clone();
            if moved {
                terminal.clear()?;
                terminal.draw(|f| ui::draw(f, app, &mut media, &mut hits, tz_offset))?;
            }
        }
        let _ = execute!(std::io::stdout(), EndSynchronizedUpdate);
        if app.bell {
            app.bell = false;
            let mut out = std::io::stdout();
            // BEL plus OSC 9, which terminals like Windows Terminal surface as a notification.
            let _ = write!(out, "\x07\x1b]9;Murk: new highlight\x07");
            let _ = out.flush();
        }
        if app.quit {
            // Wait for every network to finish its QUIT, bounded so a dead link cannot hang exit.
            let _ = tokio::time::timeout(
                murk_core::engine::QUIT_DRAIN + Duration::from_secs(3),
                async {
                    while app
                        .networks
                        .values()
                        .any(|n| n.state != murk_core::ConnState::Disconnected)
                    {
                        match events.recv().await {
                            Ok(e) => app.handle_event(e),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(_) => break,
                        }
                    }
                },
            )
            .await;
            return Ok(());
        }

        tokio::select! {
            ev = events.recv() => match ev {
                Ok(e) => {
                    app.handle_event(e);
                    // Drain whatever else is queued before redrawing.
                    while let Ok(e) = events.try_recv() {
                        app.handle_event(e);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    app.set_status(format!("UI fell behind, {n} events dropped"));
                }
                Err(_) => return Ok(()),
            },
            ev = input.next() => match ev {
                Some(Ok(e)) => handle_term(app, &mut media, &hits, e),
                Some(Err(e)) => return Err(e),
                None => return Ok(()),
            },
            Some(update) = media.rx.recv() => {
                media.apply(update);
                // Frames arrive in bursts while a video plays; take what is queued.
                while let Ok(u) = media.rx.try_recv() {
                    media.apply(u);
                }
            }
            _ = tick.tick() => app.tick(),
        }
    }
}

fn handle_term(app: &mut App, media: &mut media::Media, hits: &ui::Hits, ev: TermEvent) {
    match ev {
        TermEvent::Key(k) if k.kind != KeyEventKind::Release => handle_key(app, media, k),
        TermEvent::Paste(s) => {
            // Multi-line pastes become one message per line, sent on Enter.
            app.editor
                .insert_str(&s.replace("\r\n", "\n").replace('\n', " "));
        }
        TermEvent::Mouse(m) => match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                // The wheel over the nick list scrolls nothing else.
                if inside(hits.nicklist, m.column, m.row) {
                    return;
                }
                if let Some(b) = app.buffers.get_mut(app.active) {
                    if m.kind == MouseEventKind::ScrollUp {
                        b.scroll += 3;
                    } else {
                        b.scroll = b.scroll.saturating_sub(3);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => click(app, media, hits, m.column, m.row),
            _ => {}
        },
        _ => {}
    }
}

fn inside(r: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// A left click: a buffer in the sidebar switches to it, a nick opens a private
/// conversation, a link or picture in the chat opens with the system's handler.
fn click(app: &mut App, media: &media::Media, hits: &ui::Hits, x: u16, y: u16) {
    if inside(hits.sidebar, x, y) {
        if let Some((_, i)) = hits.buffers.iter().find(|(row, _)| *row == y) {
            app.switch(*i);
        }
        return;
    }
    if inside(hits.nicklist, x, y) {
        if let Some((_, nick)) = hits.nicks.iter().find(|(row, _)| *row == y) {
            app.submit_text(&format!("/query {nick}"));
        }
        return;
    }
    if !inside(hits.chat_area, x, y) {
        return;
    }
    if let Some((url, _)) = media.drawn.iter().find(|(_, r)| inside(*r, x, y)) {
        open_external(url);
        return;
    }
    let Some((_, left, text, links)) = hits.chat.iter().find(|(row, ..)| *row == y) else {
        return;
    };
    if links.is_empty() {
        return;
    }
    // The run of non-space text under the pointer; a wrapped link shows only
    // part of itself on a row, so match it as a piece of a link.
    let col = (x - left) as usize;
    let mut start = None;
    let mut word = String::new();
    let mut at = 0usize;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if ch.is_whitespace() {
            if start.is_some_and(|s| s <= col && col < at) {
                break;
            }
            start = None;
            word.clear();
        } else {
            if start.is_none() {
                start = Some(at);
            }
            word.push(ch);
        }
        at += w;
    }
    if !start.is_some_and(|s| s <= col && col < at) {
        return;
    }
    let piece = word.trim_matches(|c: char| ".,);:!?'\"".contains(c));
    if piece.len() < 4 {
        return;
    }
    if let Some(url) = links.iter().find(|u| u.contains(piece)) {
        open_external(url);
        app.set_status(format!("Opened {url}"));
    }
}

/// Hand a URL to whatever the system opens it with.
fn open_external(url: &str) {
    // Not `cmd /c start`: cmd would re-parse the URL, and an `&` in a query
    // string would end the command there and run the rest.
    #[cfg(windows)]
    let cmd = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let cmd = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = std::process::Command::new("xdg-open").arg(url).spawn();
    let _ = cmd;
}

fn handle_key(app: &mut App, media: &mut media::Media, k: KeyEvent) {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let page = 10;
    let scroll = |app: &mut App, up: bool| {
        if let Some(b) = app.buffers.get_mut(app.active) {
            b.scroll = if up {
                b.scroll + page
            } else {
                b.scroll.saturating_sub(page)
            };
        }
    };
    let mut edited = false;
    match k.code {
        KeyCode::Char('c') if ctrl => {
            if app.editor.is_empty() {
                app.submit_quit();
            } else {
                app.editor.home();
                app.editor.kill_to_end();
            }
        }
        KeyCode::Char(d @ '1'..='9') if alt => app.switch(d as usize - '1' as usize),
        KeyCode::Char('0') if alt => app.switch(9),
        KeyCode::Char('a') if alt => app.switch_activity(),
        KeyCode::Char('p') if ctrl => app.switch_relative(-1),
        KeyCode::Char('n') if ctrl => app.switch_relative(1),
        KeyCode::Left if alt => app.switch_relative(-1),
        KeyCode::Right if alt => app.switch_relative(1),
        KeyCode::Up if alt => app.switch_relative(-1),
        KeyCode::Down if alt => app.switch_relative(1),
        KeyCode::Char('a') if ctrl => app.editor.home(),
        KeyCode::Char('e') if ctrl => app.editor.end(),
        KeyCode::Char('u') if ctrl => app.editor.kill_to_start(),
        KeyCode::Char('k') if ctrl => app.editor.kill_to_end(),
        KeyCode::Char('w') if ctrl => app.editor.kill_word(),
        KeyCode::Char('l') if ctrl => {}
        KeyCode::Char('b') if alt => app.editor.word_left(),
        KeyCode::Char('f') if alt => app.editor.word_right(),
        KeyCode::Left if ctrl => app.editor.word_left(),
        KeyCode::Right if ctrl => app.editor.word_right(),
        // mIRC formatting shortcuts.
        KeyCode::Char('b') if ctrl => app.editor.insert('\x02'),
        KeyCode::Char('o') if ctrl => app.editor.insert('\x0f'),
        KeyCode::Char('r') if ctrl => app.editor.insert('\x16'),
        KeyCode::Char('_') if ctrl => app.editor.insert('\x1f'),
        KeyCode::Char('i') if ctrl && k.modifiers.contains(KeyModifiers::SHIFT) => {
            app.editor.insert('\x1d')
        }
        KeyCode::Char(c) if !ctrl => {
            app.editor.insert(c);
            edited = true;
        }
        KeyCode::Backspace => app.editor.backspace(),
        KeyCode::Delete => app.editor.delete(),
        KeyCode::Left => app.editor.left(),
        KeyCode::Right => app.editor.right(),
        KeyCode::Home => app.editor.home(),
        KeyCode::End => app.editor.end(),
        KeyCode::Up => app.editor.history_up(),
        KeyCode::Down => app.editor.history_down(),
        KeyCode::PageUp => scroll(app, true),
        KeyCode::PageDown => scroll(app, false),
        KeyCode::Tab => {
            let words = app
                .editor
                .completion_prefix()
                .map(|p| app.completions(&p))
                .unwrap_or_default();
            app.editor.complete(|_| words);
        }
        KeyCode::Enter => {
            // `/play` belongs to the terminal: it plays the newest video here, in place.
            if app.editor.text().trim().eq_ignore_ascii_case("/play") {
                app.editor.take();
                let found = app.current().and_then(|b| {
                    media.newest_video(b.lines.iter().rev().map(|l| l.text.as_str()))
                });
                let msg = match found {
                    Some(url) => media.toggle_play(&url),
                    None => "No video in this conversation to play".into(),
                };
                app.set_status(msg);
                return;
            }
            if let Some(b) = app.buffers.get_mut(app.active) {
                b.scroll = 0;
            }
            app.submit();
        }
        KeyCode::Esc => {
            if let Some(b) = app.buffers.get_mut(app.active) {
                b.scroll = 0;
            }
        }
        _ => {}
    }
    if edited {
        app.typing_changed();
    }
}
