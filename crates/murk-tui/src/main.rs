//! `murk`: the terminal IRC client.

mod app;
mod editor;
mod ui;

use app::App;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as TermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use futures::StreamExt;
use murk_config::Config;
use murk_core::Engine;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

const USAGE: &str = "Usage: murk [--config <path>] [--no-log] [network...]

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

#[tokio::main]
async fn main() {
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
    let mut events = engine.subscribe();
    let mut input = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        terminal.draw(|f| ui::draw(f, app, tz_offset))?;
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
                Some(Ok(e)) => handle_term(app, e),
                Some(Err(e)) => return Err(e),
                None => return Ok(()),
            },
            _ = tick.tick() => app.tick(),
        }
    }
}

fn handle_term(app: &mut App, ev: TermEvent) {
    match ev {
        TermEvent::Key(k) if k.kind != KeyEventKind::Release => handle_key(app, k),
        TermEvent::Paste(s) => {
            // Multi-line pastes become one message per line, sent on Enter.
            app.editor
                .insert_str(&s.replace("\r\n", "\n").replace('\n', " "));
        }
        TermEvent::Mouse(m) => {
            if let Some(b) = app.buffers.get_mut(app.active) {
                match m.kind {
                    MouseEventKind::ScrollUp => b.scroll += 3,
                    MouseEventKind::ScrollDown => b.scroll = b.scroll.saturating_sub(3),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn handle_key(app: &mut App, k: KeyEvent) {
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
