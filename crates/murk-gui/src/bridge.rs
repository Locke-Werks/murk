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
        #[qproperty(QString, topic)]
        #[qproperty(QString, nicks)]
        #[qproperty(QString, status)]
        #[qproperty(QString, typing)]
        #[qproperty(QString, nick)]
        type Controller = super::ControllerRust;

        /// The whole active buffer changed: a JSON array of rich-text lines.
        #[qsignal]
        fn lines_reset(self: Pin<&mut Controller>, lines: QString);

        /// Lines appended to the active buffer, as a JSON array.
        #[qsignal]
        fn lines_appended(self: Pin<&mut Controller>, lines: QString);

        /// Every network has finished disconnecting after a quit.
        #[qsignal]
        fn finished(self: Pin<&mut Controller>);

        /// A highlight arrived while the window may not be focused.
        #[qsignal]
        fn alert(self: Pin<&mut Controller>, text: QString);

        #[qinvokable]
        fn start(self: Pin<&mut Controller>);

        #[qinvokable]
        fn submit(self: Pin<&mut Controller>, text: &QString);

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

        /// Send QUIT everywhere; `finished` fires when the links are down.
        #[qinvokable]
        fn shutdown(self: Pin<&mut Controller>);
    }

    impl cxx_qt::Threading for Controller {}
}

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use murk_client::App;
use murk_core::{ConnState, Engine, Event};
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct ControllerRust {
    buffers: QString,
    active: i32,
    title: QString,
    topic: QString,
    nicks: QString,
    status: QString,
    typing: QString,
    nick: QString,
    app: Option<App>,
    /// What the view currently shows, to send appends instead of full resets.
    shown: Shown,
}

#[derive(Default, PartialEq)]
struct Shown {
    buffer: Option<(u32, Option<String>)>,
    len: usize,
    last: Option<(SystemTime, String)>,
}

fn json_str(v: serde_json::Value) -> QString {
    QString::from(v.to_string().as_str())
}

fn timestamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
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
                self.as_mut()
                    .set_status(QString::from(&format!("Config error: {e}")));
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
        if app.buffers.is_empty() {
            app.set_status(format!(
                "No networks set to autoconnect. Use /connect <name> or edit {}",
                path.display()
            ));
        }
        self.as_mut().rust_mut().get_mut().app = Some(app);

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
                    let queued = qt.queue(move |mut c| c.as_mut().apply(batch));
                    if queued.is_err() {
                        return;
                    }
                }
            });
        });
        self.refresh();
    }

    fn apply(mut self: Pin<&mut Self>, batch: Vec<Event>) {
        let Some(app) = self.as_mut().app_mut() else {
            return;
        };
        let mut alert = None;
        for e in batch {
            if let Event::Line(l) = &e
                && l.highlight
                && !l.own
                && !l.history
            {
                alert = Some(format!(
                    "{}: {}",
                    l.sender.as_deref().unwrap_or(""),
                    murk_proto::format::strip(&l.text)
                ));
            }
            app.handle_event(e);
        }
        app.bell = false;
        let done = app.quit
            && app
                .networks
                .values()
                .all(|n| n.state == ConnState::Disconnected);
        if let Some(a) = alert {
            self.as_mut().alert(QString::from(&a));
        }
        self.as_mut().refresh();
        if done {
            self.finished();
        }
    }

    /// Push the app state into properties, and new lines into the view.
    fn refresh(mut self: Pin<&mut Self>) {
        let prev = std::mem::take(&mut self.as_mut().rust_mut().get_mut().shown);
        let Some(app) = self.as_mut().app_mut() else {
            return;
        };
        let active = app.active;
        let mut list = Vec::new();
        for (i, b) in app.buffers.iter().enumerate() {
            let net = app.networks.get(&b.network);
            list.push(serde_json::json!({
                "name": b.name.clone().unwrap_or_else(|| net.map(|n| n.name.clone()).unwrap_or_default()),
                "server": b.name.is_none(),
                "unread": b.unread,
                "highlight": b.highlighted,
                "joined": b.joined || b.name.is_none() || !b.is_channel_like(),
                "connected": net.is_some_and(|n| n.state == ConnState::Connected),
                "active": i == active,
            }));
        }
        let buffers = json_str(serde_json::Value::Array(list));

        let mut title = String::new();
        let mut topic = String::new();
        let mut nicks = serde_json::Value::Array(vec![]);
        let mut own_nick = String::new();
        let mut typing = String::new();
        let mut shown = Shown::default();
        let mut reset = None;
        let mut appended = None;
        if let Some(b) = app.buffers.get(active) {
            let net = app.networks.get(&b.network);
            own_nick = net.map(|n| n.nick.clone()).unwrap_or_default();
            title = match (&b.name, net) {
                (Some(n), Some(net)) => format!("{n} \u{b7} {}", net.name),
                (None, Some(net)) => match &net.detail {
                    Some(d) if net.state == ConnState::Disconnected => {
                        format!("{} (disconnected: {d})", net.name)
                    }
                    _ => net.name.clone(),
                },
                (Some(n), None) => n.clone(),
                (None, None) => String::new(),
            };
            if let Some(name) = &b.name {
                let st = app.engine.with_state(b.network, |s| {
                    let topic = s.channel(name).and_then(|c| c.topic.clone());
                    let members: Vec<_> = s
                        .sorted_members(name)
                        .into_iter()
                        .map(|m| {
                            serde_json::json!({
                                "prefix": m.highest_prefix().map(String::from).unwrap_or_default(),
                                "nick": m.nick,
                                "color": crate::html::nick_color(&m.nick),
                                "away": m.away,
                            })
                        })
                        .collect();
                    (topic, members)
                });
                if let Some((t, m)) = st {
                    topic = t.map(|t| crate::html::styled(&t)).unwrap_or_default();
                    nicks = serde_json::Value::Array(m);
                }
            }
            if !b.typing.is_empty() {
                let mut who: Vec<&str> = b.typing.keys().map(String::as_str).collect();
                who.sort();
                typing = format!(
                    "{} {} typing\u{2026}",
                    who.join(", "),
                    if who.len() == 1 { "is" } else { "are" }
                );
            }
            shown = Shown {
                buffer: Some((b.network, b.name.clone())),
                len: b.lines.len(),
                last: b.lines.back().map(|l| (l.time, l.text.clone())),
            };
            let render = |l: &murk_core::Line| crate::html::line(l, &timestamp(l.time));
            if prev.buffer != shown.buffer || prev.last.is_none() {
                reset = Some(b.lines.iter().map(render).collect::<Vec<_>>());
            } else if prev != shown {
                // Find the last line the view has and send what follows it.
                let pos = b.lines.iter().rev().take(2000).position(|l| {
                    prev.last
                        .as_ref()
                        .is_some_and(|(t, s)| *t == l.time && *s == l.text)
                });
                match pos {
                    Some(n) if b.lines.len() >= prev.len => {
                        let start = b.lines.len() - n;
                        appended = Some(b.lines.range(start..).map(render).collect::<Vec<_>>());
                    }
                    _ => reset = Some(b.lines.iter().map(render).collect::<Vec<_>>()),
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
        this.as_mut().set_buffers(buffers);
        this.as_mut().set_active(active as i32);
        this.as_mut().set_title(QString::from(&title));
        this.as_mut().set_topic(QString::from(&topic));
        this.as_mut().set_nicks(json_str(nicks));
        this.as_mut().set_status(QString::from(&status));
        this.as_mut().set_typing(QString::from(&typing));
        this.as_mut().set_nick(QString::from(&own_nick));
        if let Some(r) = reset {
            this.as_mut().lines_reset(json_str(r.into()));
        } else if let Some(a) = appended
            && !a.is_empty()
        {
            this.as_mut().lines_appended(json_str(a.into()));
        }
    }

    pub fn submit(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        if let Some(app) = self.as_mut().app_mut() {
            app.editor.set(text);
            app.submit();
        }
        self.refresh();
    }

    pub fn switch_to(mut self: Pin<&mut Self>, index: i32) {
        if let Some(app) = self.as_mut().app_mut() {
            app.switch(index.max(0) as usize);
        }
        self.refresh();
    }

    pub fn switch_relative(mut self: Pin<&mut Self>, delta: i32) {
        if let Some(app) = self.as_mut().app_mut() {
            app.switch_relative(delta as isize);
        }
        self.refresh();
    }

    pub fn switch_activity(mut self: Pin<&mut Self>) {
        if let Some(app) = self.as_mut().app_mut() {
            app.switch_activity();
        }
        self.refresh();
    }

    pub fn load_older(mut self: Pin<&mut Self>) {
        if let Some(app) = self.as_mut().app_mut() {
            app.load_older();
        }
        // Prepends invalidate positions; force a full reset.
        self.as_mut().rust_mut().get_mut().shown.last = None;
        self.refresh();
    }

    pub fn tick(mut self: Pin<&mut Self>) {
        if let Some(app) = self.as_mut().app_mut() {
            app.tick();
        }
        self.refresh();
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
            return QString::from(&text);
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
        QString::from(app.editor.text())
    }

    pub fn history(mut self: Pin<&mut Self>, text: &QString, up: bool) -> QString {
        let text = text.to_string();
        let Some(app) = self.as_mut().app_mut() else {
            return QString::from(&text);
        };
        if app.editor.text() != text {
            app.editor.set(text);
        }
        if up {
            app.editor.history_up();
        } else {
            app.editor.history_down();
        }
        QString::from(app.editor.text())
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
