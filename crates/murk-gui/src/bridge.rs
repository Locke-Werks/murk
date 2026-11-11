//! The QObject QML talks to. It owns the shared client `App` on the Qt thread and
//! receives engine events from a pump thread through `qt_thread().queue`.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, buffers)]
        #[qproperty(i32, active)]
        #[qproperty(QString, title)]
        #[qproperty(QString, network)]
        #[qproperty(QString, topic)]
        #[qproperty(QString, modes)]
        #[qproperty(QString, nicks)]
        #[qproperty(QString, status)]
        #[qproperty(QString, typing)]
        #[qproperty(QString, nick)]
        #[qproperty(QString, kind)]
        #[qproperty(QString, settings)]
        #[qproperty(bool, ready)]
        type Controller = super::ControllerRust;

        /// The whole active buffer changed: a JSON array of line objects.
        #[qsignal]
        fn lines_reset(self: Pin<&mut Controller>, lines: QString);

        /// Lines appended to the active buffer, as a JSON array.
        #[qsignal]
        fn lines_appended(self: Pin<&mut Controller>, lines: QString);

        /// One line of the active buffer changed in place (reactions).
        #[qsignal]
        fn line_changed(self: Pin<&mut Controller>, index: i32, line: QString);

        /// Every network has finished disconnecting after a quit.
        #[qsignal]
        fn finished(self: Pin<&mut Controller>);

        /// A link preview finished: JSON card, or `null` when there is nothing to show.
        #[qsignal]
        fn preview_ready(self: Pin<&mut Controller>, url: QString, preview: QString);

        /// Someone mentioned us or messaged us privately.
        #[qsignal]
        fn alert(self: Pin<&mut Controller>, title: QString, text: QString, index: i32);

        #[qinvokable]
        fn start(self: Pin<&mut Controller>);

        #[qinvokable]
        fn submit(self: Pin<&mut Controller>, text: &QString);

        #[qinvokable]
        fn reply(self: Pin<&mut Controller>, msgid: &QString, text: &QString);

        #[qinvokable]
        fn react(self: Pin<&mut Controller>, msgid: &QString, emoji: &QString);

        #[qinvokable]
        fn switch_to(self: Pin<&mut Controller>, index: i32);

        #[qinvokable]
        fn switch_relative(self: Pin<&mut Controller>, delta: i32);

        #[qinvokable]
        fn switch_activity(self: Pin<&mut Controller>);

        #[qinvokable]
        fn load_older(self: Pin<&mut Controller>);

        #[qinvokable]
        fn tick(self: Pin<&mut Controller>);

        #[qinvokable]
        fn edited(self: Pin<&mut Controller>, text: &QString);

        /// Tab completion at the end of `text`; returns the new text.
        #[qinvokable]
        fn complete(self: Pin<&mut Controller>, text: &QString) -> QString;

        /// Input history; returns the recalled text.
        #[qinvokable]
        fn history(self: Pin<&mut Controller>, text: &QString, up: bool) -> QString;

        /// Emoji catalog for the picker: JSON `[{code, emoji, category}]`.
        #[qinvokable]
        fn emoji_catalog(self: &Controller) -> QString;

        #[qinvokable]
        fn quick_reactions(self: &Controller) -> QString;

        /// Full-text search of the log: JSON array of hits, newest first.
        #[qinvokable]
        fn search(self: &Controller, query: &QString) -> QString;

        /// Every logged line that highlighted us: JSON array, newest first.
        #[qinvokable]
        fn mentions(self: &Controller) -> QString;

        /// Switch to a logged buffer by network and buffer name.
        #[qinvokable]
        fn open_stored(self: Pin<&mut Controller>, network: &QString, buffer: &QString) -> bool;

        /// Fetch a link preview in the background; answers with `preview_ready`.
        #[qinvokable]
        fn request_preview(self: Pin<&mut Controller>, url: &QString);

        /// Slash commands with usage: JSON `[{name, usage}]`.
        #[qinvokable]
        fn commands(self: &Controller) -> QString;

        /// Show a desktop notification.
        #[qinvokable]
        fn notify(self: &Controller, title: &QString, text: &QString);

        /// Change one UI setting and persist it. `value` is JSON.
        #[qinvokable]
        fn set_setting(self: Pin<&mut Controller>, key: &QString, value: &QString);

        /// Configured networks, secrets omitted: JSON array.
        #[qinvokable]
        fn network_list(self: &Controller) -> QString;

        /// Add or replace a network from JSON, save, and connect it if `connect`.
        #[qinvokable]
        fn save_network(self: Pin<&mut Controller>, json: &QString, connect: bool) -> QString;

        #[qinvokable]
        fn remove_network(self: Pin<&mut Controller>, name: &QString);

        #[qinvokable]
        fn config_path(self: &Controller) -> QString;

        /// Send QUIT everywhere; `finished` fires when the links are down.
        #[qinvokable]
        fn shutdown(self: Pin<&mut Controller>);
    }

    impl cxx_qt::Threading for Controller {}
}

use crate::model;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use murk_client::App;
use murk_core::{ConnState, Engine, Event, LineKind};
use std::path::PathBuf;
use std::pin::Pin;
use std::time::SystemTime;

#[derive(Default)]
pub struct ControllerRust {
    buffers: QString,
    active: i32,
    title: QString,
    network: QString,
    topic: QString,
    modes: QString,
    nicks: QString,
    status: QString,
    typing: QString,
    nick: QString,
    kind: QString,
    settings: QString,
    ready: bool,
    app: Option<App>,
    path: PathBuf,
    /// What the view currently shows, to send appends instead of full resets.
    shown: Shown,
    /// msgids whose reactions changed since the last refresh.
    dirty: Vec<String>,
    previewer: Option<murk_media::Previewer>,
}

#[derive(Default, PartialEq, Clone)]
struct Shown {
    buffer: Option<(u32, Option<String>)>,
    rev: u64,
    len: usize,
    last: Option<(SystemTime, String)>,
}

fn qs(s: &str) -> QString {
    QString::from(s)
}

fn json_str(v: &serde_json::Value) -> QString {
    QString::from(v.to_string().as_str())
}

/// Logged lines for the search and mentions views. Matches of `query` words are marked.
fn stored_json(hits: &[murk_store::Stored], query: Option<&str>) -> serde_json::Value {
    let words: Vec<String> = query
        .map(|q| {
            q.split_whitespace()
                .map(|w| w.trim_matches('"').to_lowercase())
                .filter(|w| w.len() > 1)
                .collect()
        })
        .unwrap_or_default();
    let mark = |text: &str| -> String {
        let plain = murk_proto::format::strip(text);
        if words.is_empty() {
            return crate::html::escape(&plain);
        }
        // Mark case-insensitively on char boundaries of the original text.
        let lower = plain.to_lowercase();
        if lower.len() != plain.len() {
            return crate::html::escape(&plain);
        }
        let mut out = String::new();
        let mut i = 0;
        while i < plain.len() {
            let hit = words.iter().find(|w| {
                lower[i..].starts_with(w.as_str()) && plain.is_char_boundary(i + w.len())
            });
            match hit {
                Some(w) => {
                    out.push_str(&format!(
                        "<b><u>{}</u></b>",
                        crate::html::escape(&plain[i..i + w.len()])
                    ));
                    i += w.len();
                }
                None => {
                    let c = plain[i..].chars().next().unwrap();
                    out.push_str(&crate::html::escape(&c.to_string()));
                    i += c.len_utf8();
                }
            }
        }
        out
    };
    let v: Vec<_> = hits
        .iter()
        .map(|s| {
            let sender = s.sender.clone().unwrap_or_default();
            let ms = s
                .time
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let t = chrono::DateTime::from_timestamp_millis(ms)
                .unwrap_or_default()
                .with_timezone(&chrono::Local);
            serde_json::json!({
                "network": s.network,
                "buffer": s.buffer,
                "sender": sender,
                "initial": model::initial(&sender),
                "color": crate::html::nick_color(&sender),
                "html": mark(&s.text),
                "when": t.format("%b %-d, %H:%M").to_string(),
            })
        })
        .collect();
    v.into()
}

fn settings_json(app: &App) -> serde_json::Value {
    let ui = &app.config.ui;
    serde_json::json!({
        "theme": ui.theme,
        "layout": ui.layout,
        "fontSize": ui.font_size,
        "showJoinsParts": ui.show_joins_parts,
        "nicklist": ui.nicklist,
        "highlights": ui.highlights,
        "inlineMedia": ui.inline_media,
    })
}

impl qobject::Controller {
    fn app_mut(self: Pin<&mut Self>) -> Option<&mut App> {
        self.rust_mut().get_mut().app.as_mut()
    }

    pub fn start(mut self: Pin<&mut Self>) {
        if self.app.is_some() {
            return;
        }
        let path = crate::config_path();
        let config = match murk_config::Config::load(&path) {
            Ok(c) => c,
            Err(e) => {
                self.as_mut().set_status(qs(&format!("Config error: {e}")));
                return;
            }
        };
        if !path.exists() {
            let _ = config.save(&path);
        }
        let dir = murk_config::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let store = murk_store::Store::open(dir.join("logs.db")).ok();
        let engine = Engine::new();
        let mut events = engine.subscribe();
        let mut app = App::new(engine, store, config);
        let startup: Vec<String> = app
            .config
            .networks
            .iter()
            .filter(|n| n.autoconnect)
            .map(|n| n.name.clone())
            .collect();
        for name in startup {
            if let Err(e) = app.connect(&name) {
                app.set_status(e);
            }
        }
        let settings = json_str(&settings_json(&app));
        {
            let r = self.as_mut().rust_mut().get_mut();
            r.app = Some(app);
            r.path = path;
        }
        self.as_mut().set_settings(settings);

        // Batch events so a burst (joins, NAMES, playback) is one UI update.
        let qt = self.qt_thread();
        let rt = crate::runtime();
        std::thread::spawn(move || {
            rt.block_on(async move {
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    let first = match events.recv().await {
                        Ok(e) => e,
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => return,
                    };
                    let mut batch = vec![first];
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                    while let Ok(e) = events.try_recv() {
                        batch.push(e);
                    }
                    if qt.queue(move |mut c| c.as_mut().apply(batch)).is_err() {
                        return;
                    }
                }
            });
        });
        self.as_mut().set_ready(true);
        self.refresh();
    }

    fn apply(mut self: Pin<&mut Self>, batch: Vec<Event>) {
        let mut dirty = Vec::new();
        let mut alert = None;
        {
            let Some(app) = self.as_mut().app_mut() else {
                return;
            };
            for e in batch {
                match &e {
                    Event::Line(l)
                        if !l.own
                            && !l.history
                            && matches!(
                                l.kind,
                                LineKind::Privmsg | LineKind::Action | LineKind::Notice
                            ) =>
                    {
                        let query = l.buffer.as_deref().is_some_and(|b| {
                            !b.starts_with(['#', '&', '!', '+'])
                                && l.sender
                                    .as_deref()
                                    .is_some_and(|s| s.eq_ignore_ascii_case(b))
                        });
                        if l.highlight || query {
                            let sender = l.sender.clone().unwrap_or_default();
                            let title = match (&l.buffer, query) {
                                (Some(b), false) => format!("{sender} in {b}"),
                                _ => sender.clone(),
                            };
                            alert = Some((
                                title,
                                murk_proto::format::strip(&l.text),
                                l.network,
                                l.buffer.clone(),
                            ));
                        }
                    }
                    Event::React { msgid, .. } => dirty.push(msgid.clone()),
                    _ => {}
                }
                app.handle_event(e);
            }
            app.bell = false;
        }
        self.as_mut().rust_mut().get_mut().dirty.extend(dirty);
        if let Some((title, text, net, buf)) = alert {
            let idx = self
                .as_mut()
                .app_mut()
                .map(|a| a.buffer_index(net, buf.as_deref()) as i32)
                .unwrap_or(-1);
            self.as_mut().alert(qs(&title), qs(&text), idx);
        }
        self.as_mut().refresh();
        let done = self.app.as_ref().is_some_and(|app| {
            app.quit
                && app
                    .networks
                    .values()
                    .all(|n| n.state == ConnState::Disconnected)
        });
        if done {
            self.finished();
        }
    }

    /// Push the app state into properties, and new lines into the view.
    fn refresh(mut self: Pin<&mut Self>) {
        let prev = self.shown.clone();
        let dirty = std::mem::take(&mut self.as_mut().rust_mut().get_mut().dirty);
        let Some(app) = self.as_mut().app_mut() else {
            return;
        };
        let active = app.active;
        let mut list = Vec::new();
        let mut last_net = None;
        for (i, b) in app.buffers.iter().enumerate() {
            let net = app.networks.get(&b.network);
            let net_name = net.map(|n| n.name.clone()).unwrap_or_default();
            list.push(serde_json::json!({
                "index": i,
                "name": b.name.clone().unwrap_or_else(|| net_name.clone()),
                "network": net_name,
                "firstOfNetwork": last_net != Some(b.network),
                "server": b.name.is_none(),
                "channel": b.is_channel_like(),
                "unread": b.unread,
                "highlight": b.highlighted,
                "joined": b.joined || b.name.is_none() || !b.is_channel_like(),
                "state": match net.map(|n| n.state) {
                    Some(ConnState::Connected) => "connected",
                    Some(ConnState::Connecting) | Some(ConnState::Registering) => "connecting",
                    _ => "disconnected",
                },
                "active": i == active,
                "typing": !b.typing.is_empty(),
            }));
            last_net = Some(b.network);
        }
        let buffers = json_str(&serde_json::Value::Array(list));

        let mut title = String::new();
        let mut network = String::new();
        let mut topic = String::new();
        let mut modes = String::new();
        let mut kind = "none";
        let mut nicks = serde_json::Value::Array(vec![]);
        let mut own_nick = String::new();
        let mut typing = String::new();
        let mut shown = Shown::default();
        let mut reset = None;
        let mut appended = None;
        let mut changed = Vec::new();
        if let Some(b) = app.buffers.get(active) {
            let net = app.networks.get(&b.network);
            own_nick = net.map(|n| n.nick.clone()).unwrap_or_default();
            network = net.map(|n| n.name.clone()).unwrap_or_default();
            kind = if b.name.is_none() {
                "server"
            } else if b.is_channel_like() {
                "channel"
            } else {
                "query"
            };
            title = match (&b.name, net) {
                (Some(n), _) => n.clone(),
                (None, Some(net)) => net.name.clone(),
                (None, None) => String::new(),
            };
            if b.name.is_none()
                && let Some(n) = net
                && n.state == ConnState::Disconnected
            {
                topic = match &n.detail {
                    Some(d) => format!("Disconnected: {}", crate::html::escape(d)),
                    None => "Disconnected".into(),
                };
            }
            if let Some(name) = &b.name {
                let st = app.engine.with_state(b.network, |s| {
                    let ch = s.channel(name);
                    let topic = ch.and_then(|c| c.topic.clone());
                    let modes = ch.map(|c| c.modes.clone()).unwrap_or_default();
                    let members: Vec<_> = s
                        .sorted_members(name)
                        .into_iter()
                        .map(|m| {
                            serde_json::json!({
                                "prefix": m.highest_prefix().map(String::from).unwrap_or_default(),
                                "nick": m.nick,
                                "initial": model::initial(&m.nick),
                                "color": crate::html::nick_color(&m.nick),
                                "away": m.away,
                            })
                        })
                        .collect();
                    (topic, modes, members)
                });
                if let Some((t, m, list)) = st {
                    topic = t.map(|t| crate::html::styled(&t)).unwrap_or_default();
                    modes = if m.is_empty() { m } else { format!("+{m}") };
                    nicks = serde_json::Value::Array(list);
                }
            }
            if !b.typing.is_empty() {
                let mut who: Vec<&str> = b.typing.keys().map(String::as_str).collect();
                who.sort();
                typing = match who.len() {
                    1 => format!("{} is typing", who[0]),
                    2 => format!("{} and {} are typing", who[0], who[1]),
                    n => format!("{n} people are typing"),
                };
            }
            shown = Shown {
                buffer: Some((b.network, b.name.clone())),
                rev: b.rev,
                len: b.lines.len(),
                last: b.lines.back().map(|l| (l.time, l.text.clone())),
            };
            let me = own_nick.as_str();
            let all = || {
                (0..b.lines.len())
                    .map(|i| model::line(b, i, me))
                    .collect::<Vec<_>>()
            };
            if prev.buffer != shown.buffer || prev.last.is_none() {
                reset = Some(all());
            } else if prev.rev != shown.rev {
                if prev.last != shown.last {
                    // Find the last line the view has and send what follows it.
                    let pos = b.lines.iter().rev().take(2000).position(|l| {
                        prev.last
                            .as_ref()
                            .is_some_and(|(t, s)| *t == l.time && *s == l.text)
                    });
                    match pos {
                        Some(n) if b.lines.len() >= prev.len => {
                            let start = b.lines.len() - n;
                            appended = Some(
                                (start..b.lines.len())
                                    .map(|i| model::line(b, i, me))
                                    .collect::<Vec<_>>(),
                            );
                            // The line before may have been the end of a membership run that now continues.
                            if start > 0
                                && start < b.lines.len()
                                && model::is_foldable(&b.lines[start - 1])
                                && model::is_foldable(&b.lines[start])
                            {
                                changed.push(((start - 1) as i32, model::line(b, start - 1, me)));
                            }
                        }
                        _ => reset = Some(all()),
                    }
                } else if prev.len != shown.len {
                    reset = Some(all());
                }
                if reset.is_none() {
                    for id in &dirty {
                        if let Some(i) =
                            b.lines.iter().rposition(|l| l.msgid.as_deref() == Some(id))
                        {
                            changed.push((i as i32, model::line(b, i, me)));
                        }
                    }
                }
            }
        } else if prev.buffer.is_some() {
            reset = Some(vec![]);
        }
        let status = app
            .status
            .as_ref()
            .map(|(s, _)| s.clone())
            .unwrap_or_default();

        let mut this = self.as_mut();
        this.as_mut().rust_mut().get_mut().shown = shown;
        if *this.buffers() != buffers {
            this.as_mut().set_buffers(buffers);
        }
        this.as_mut().set_active(active as i32);
        this.as_mut().set_title(qs(&title));
        this.as_mut().set_network(qs(&network));
        this.as_mut().set_topic(qs(&topic));
        this.as_mut().set_modes(qs(&modes));
        let nicks = json_str(&nicks);
        if *this.nicks() != nicks {
            this.as_mut().set_nicks(nicks);
        }
        this.as_mut().set_status(qs(&status));
        this.as_mut().set_typing(qs(&typing));
        this.as_mut().set_nick(qs(&own_nick));
        this.as_mut().set_kind(qs(kind));
        if let Some(r) = reset {
            this.as_mut().lines_reset(json_str(&r.into()));
        } else if let Some(a) = appended
            && !a.is_empty()
        {
            this.as_mut().lines_appended(json_str(&a.into()));
        }
        for (i, l) in changed {
            this.as_mut().line_changed(i, json_str(&l));
        }
    }

    fn with_app(mut self: Pin<&mut Self>, f: impl FnOnce(&mut App)) {
        if let Some(app) = self.as_mut().app_mut() {
            f(app);
        }
        self.refresh();
    }

    pub fn submit(self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        self.with_app(|app| {
            app.editor.set(text);
            app.submit();
        });
    }

    pub fn reply(self: Pin<&mut Self>, msgid: &QString, text: &QString) {
        let (msgid, text) = (msgid.to_string(), text.to_string());
        self.with_app(|app| {
            // Record in input history like any other line.
            app.editor.set(text.clone());
            let _ = app.editor.take();
            app.reply(&msgid, &text);
        });
    }

    pub fn react(self: Pin<&mut Self>, msgid: &QString, emoji: &QString) {
        let (msgid, emoji) = (msgid.to_string(), emoji.to_string());
        self.with_app(|app| app.react(&msgid, &emoji));
    }

    pub fn switch_to(self: Pin<&mut Self>, index: i32) {
        self.with_app(|app| app.switch(index.max(0) as usize));
    }

    pub fn switch_relative(self: Pin<&mut Self>, delta: i32) {
        self.with_app(|app| app.switch_relative(delta as isize));
    }

    pub fn switch_activity(self: Pin<&mut Self>) {
        self.with_app(|app| app.switch_activity());
    }

    pub fn load_older(mut self: Pin<&mut Self>) {
        if let Some(app) = self.as_mut().app_mut() {
            app.load_older();
        }
        // Prepends invalidate positions; force a full reset.
        self.as_mut().rust_mut().get_mut().shown.last = None;
        self.refresh();
    }

    pub fn tick(self: Pin<&mut Self>) {
        self.with_app(|app| app.tick());
    }

    pub fn edited(mut self: Pin<&mut Self>, text: &QString) {
        if let Some(app) = self.as_mut().app_mut() {
            let text = text.to_string();
            if app.editor.text() != text {
                app.editor.set(text);
            }
            app.typing_changed();
        }
    }

    pub fn complete(mut self: Pin<&mut Self>, text: &QString) -> QString {
        let text = text.to_string();
        let Some(app) = self.as_mut().app_mut() else {
            return qs(&text);
        };
        if app.editor.text() != text {
            app.editor.set(text);
        }
        let words = app
            .editor
            .completion_prefix()
            .map(|p| app.completions(&p))
            .unwrap_or_default();
        app.editor.complete(|_| words);
        qs(app.editor.text())
    }

    pub fn history(mut self: Pin<&mut Self>, text: &QString, up: bool) -> QString {
        let text = text.to_string();
        let Some(app) = self.as_mut().app_mut() else {
            return qs(&text);
        };
        if app.editor.text() != text {
            app.editor.set(text);
        }
        if up {
            app.editor.history_up();
        } else {
            app.editor.history_down();
        }
        qs(app.editor.text())
    }

    pub fn emoji_catalog(&self) -> QString {
        let v: Vec<_> = murk_client::emoji::EMOJI
            .iter()
            .map(|(c, e, cat)| serde_json::json!({ "code": c, "emoji": e, "category": cat }))
            .collect();
        json_str(&v.into())
    }

    pub fn quick_reactions(&self) -> QString {
        json_str(&murk_client::emoji::QUICK_REACTIONS.into())
    }

    pub fn search(&self, query: &QString) -> QString {
        let q = query.to_string();
        let Some(store) = self.app.as_ref().and_then(|a| a.store.as_ref()) else {
            return qs("[]");
        };
        if q.trim().is_empty() {
            return qs("[]");
        }
        let hits = store.search(&q, None, None, 200).unwrap_or_default();
        json_str(&stored_json(&hits, Some(&q)))
    }

    pub fn mentions(&self) -> QString {
        let Some(store) = self.app.as_ref().and_then(|a| a.store.as_ref()) else {
            return qs("[]");
        };
        json_str(&stored_json(&store.mentions(300).unwrap_or_default(), None))
    }

    pub fn open_stored(mut self: Pin<&mut Self>, network: &QString, buffer: &QString) -> bool {
        let (n, b) = (network.to_string(), buffer.to_string());
        let ok = self
            .as_mut()
            .app_mut()
            .is_some_and(|a| a.open_stored(&n, &b));
        self.refresh();
        ok
    }

    pub fn request_preview(mut self: Pin<&mut Self>, url: &QString) {
        if self.app.as_ref().is_some_and(|a| !a.config.ui.inline_media) {
            return;
        }
        if self.previewer.is_none() {
            let dir = murk_config::data_dir().join("media");
            let p = murk_media::Previewer::new(dir).ok();
            self.as_mut().rust_mut().get_mut().previewer = p;
        }
        let Some(p) = self.previewer.clone() else {
            return;
        };
        let url = url.to_string();
        let qt = self.qt_thread();
        crate::runtime().spawn(async move {
            let preview = p.preview(&url).await;
            let json = match preview {
                Some(p) => serde_json::json!({
                    "url": p.url,
                    "kind": p.kind,
                    "title": p.title,
                    "description": p.description,
                    "site": p.site,
                    "image": p.image.map(|i| format!("file:///{}", i.display().to_string().replace('\\', "/").trim_start_matches('/'))),
                }),
                None => serde_json::Value::Null,
            };
            let _ = qt.queue(move |c| c.preview_ready(qs(&url), json_str(&json)));
        });
    }

    pub fn commands(&self) -> QString {
        let mut v: Vec<_> = murk_core::input::HELP
            .iter()
            .map(|(n, u)| serde_json::json!({ "name": n, "usage": u }))
            .collect();
        v.push(serde_json::json!({ "name": "script", "usage": "/script [list|reload]" }));
        if let Some(s) = self.app.as_ref().and_then(|a| a.scripts.as_ref()) {
            for (n, u) in s.commands() {
                v.push(serde_json::json!({ "name": n, "usage": format!("{u}  (script)") }));
            }
        }
        json_str(&v.into())
    }

    pub fn notify(&self, title: &QString, text: &QString) {
        let (title, text) = (title.to_string(), text.to_string());
        // Toasts can block briefly while the shell answers; keep that off the UI thread.
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .appname("Murk")
                .summary(&title)
                .body(&text)
                .show();
        });
    }

    pub fn set_setting(mut self: Pin<&mut Self>, key: &QString, value: &QString) {
        let key = key.to_string();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&value.to_string()) else {
            return;
        };
        let path = self.path.clone();
        let Some(app) = self.as_mut().app_mut() else {
            return;
        };
        let ui = &mut app.config.ui;
        match (key.as_str(), &v) {
            ("theme", serde_json::Value::String(s)) => ui.theme = s.clone(),
            ("layout", serde_json::Value::String(s)) => ui.layout = s.clone(),
            ("fontSize", serde_json::Value::Number(n)) => {
                ui.font_size = n.as_u64().unwrap_or(11).clamp(8, 24) as u32
            }
            ("showJoinsParts", serde_json::Value::Bool(b)) => ui.show_joins_parts = *b,
            ("nicklist", serde_json::Value::Bool(b)) => ui.nicklist = *b,
            ("inlineMedia", serde_json::Value::Bool(b)) => ui.inline_media = *b,
            ("highlights", serde_json::Value::Array(a)) => {
                ui.highlights = a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            }
            _ => return,
        }
        if let Err(e) = app.config.save(&path) {
            app.set_status(format!("Could not save settings: {e}"));
        }
        let s = json_str(&settings_json(app));
        self.as_mut().set_settings(s);
    }

    pub fn network_list(&self) -> QString {
        let Some(app) = &self.app else {
            return qs("[]");
        };
        let v: Vec<_> = app
            .config
            .networks
            .iter()
            .map(|n| {
                let state = app
                    .networks
                    .values()
                    .find(|x| x.name.eq_ignore_ascii_case(&n.name))
                    .map(|x| x.state);
                serde_json::json!({
                    "name": n.name,
                    "host": n.host,
                    "port": n.port,
                    "tls": n.tls,
                    "nick": n.nick.clone().unwrap_or_default(),
                    "autojoin": n.autojoin.join(" "),
                    "autoconnect": n.autoconnect,
                    "hasPassword": n.password.is_some() || n.sasl.is_some(),
                    "connected": matches!(state, Some(ConnState::Connected)),
                })
            })
            .collect();
        json_str(&v.into())
    }

    pub fn save_network(mut self: Pin<&mut Self>, json: &QString, connect: bool) -> QString {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&json.to_string()) else {
            return qs("Invalid network data");
        };
        let s = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let name = s("name");
        let host = s("host");
        if name.is_empty() || host.is_empty() {
            return qs("A network needs a name and a server address");
        }
        let path = self.path.clone();
        let Some(app) = self.as_mut().app_mut() else {
            return qs("Not ready");
        };
        let original = s("original");
        let key = if original.is_empty() {
            &name
        } else {
            &original
        };
        let existing = app
            .config
            .networks
            .iter()
            .position(|n| n.name.eq_ignore_ascii_case(key));
        let mut n = existing
            .map(|i| app.config.networks[i].clone())
            .unwrap_or_default();
        n.name = name.clone();
        n.host = host;
        n.port = v.get("port").and_then(|x| x.as_u64()).unwrap_or(6697) as u16;
        n.tls = v.get("tls").and_then(|x| x.as_bool()).unwrap_or(true);
        let nick = s("nick");
        n.nick = (!nick.is_empty()).then_some(nick);
        n.autojoin = s("autojoin")
            .split([' ', ','])
            .filter(|c| !c.is_empty())
            .map(|c| {
                if c.starts_with(['#', '&']) {
                    c.to_string()
                } else {
                    format!("#{c}")
                }
            })
            .collect();
        n.autoconnect = v
            .get("autoconnect")
            .and_then(|x| x.as_bool())
            .unwrap_or(true);
        let password = s("password");
        if !password.is_empty() {
            n.password = Some(murk_config::Secret::Inline(password));
        }
        match existing {
            Some(i) => app.config.networks[i] = n,
            None => app.config.networks.push(n),
        }
        let mut msg = String::new();
        if let Err(e) = app.config.save(&path) {
            msg = format!("Could not save: {e}");
        }
        if connect && msg.is_empty() {
            let running = app
                .networks
                .values()
                .any(|x| x.name.eq_ignore_ascii_case(&name) && x.state != ConnState::Disconnected);
            if !running && let Err(e) = app.connect(&name) {
                msg = e;
            }
        }
        self.refresh();
        qs(&msg)
    }

    pub fn remove_network(mut self: Pin<&mut Self>, name: &QString) {
        let name = name.to_string();
        let path = self.path.clone();
        if let Some(app) = self.as_mut().app_mut() {
            app.config
                .networks
                .retain(|n| !n.name.eq_ignore_ascii_case(&name));
            let _ = app.config.save(&path);
        }
        self.refresh();
    }

    pub fn config_path(&self) -> QString {
        qs(&self.path.display().to_string())
    }

    pub fn shutdown(mut self: Pin<&mut Self>) {
        let done = match self.as_mut().app_mut() {
            Some(app) => {
                app.submit_quit();
                app.networks
                    .values()
                    .all(|n| n.state == ConnState::Disconnected)
            }
            None => true,
        };
        if done {
            self.finished();
        }
    }
}
