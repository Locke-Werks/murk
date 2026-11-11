//! Lua 5.4 scripting, sandboxed: no `io`, no `os.execute`, no module loading
//! from disk. Scripts talk to the client only through the `murk` table.
//!
//! ```lua
//! murk.on("message", function(m)
//!   if m.text:match("^!ping") then murk.say("pong, " .. m.nick) end
//! end)
//! murk.command("slap", function(args)
//!   murk.me("slaps " .. args .. " around a bit with a large trout")
//! end, "/slap <nick>")
//! murk.timer(60, function() murk.print("a minute passed") end)
//! ```
//!
//! Script calls never touch the network directly. They queue [`Action`]s that
//! the client drains after each hook, against the network and buffer the hook
//! ran in.

use mlua::{Function, Lua, LuaOptions, StdLib, Table, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Where a hook ran: script output goes back here unless it names a target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ctx {
    pub network: u32,
    pub buffer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Say {
        target: String,
        text: String,
    },
    Action {
        target: String,
        text: String,
    },
    Raw(String),
    Print(String),
    React {
        msgid: String,
        emoji: String,
    },
    /// Run a line as if typed, including slash commands.
    Run(String),
}

/// A message as scripts see it.
#[derive(Debug, Clone, Default)]
pub struct Message {
    pub network: String,
    pub buffer: String,
    pub nick: String,
    pub text: String,
    pub kind: &'static str,
    pub own: bool,
    pub highlight: bool,
    pub msgid: Option<String>,
}

struct Timer {
    id: u64,
    due: Instant,
    every: Option<Duration>,
    func: Function,
    ctx: Ctx,
}

#[derive(Default)]
struct State {
    hooks: HashMap<String, Vec<Function>>,
    commands: HashMap<String, (Function, String)>,
    timers: Vec<Timer>,
    next_timer: u64,
    ctx: Ctx,
    actions: Vec<(Ctx, Action)>,
    /// Script currently loading, for error messages.
    loading: Option<String>,
}

pub struct ScriptHost {
    lua: Lua,
    state: Rc<RefCell<State>>,
    loaded: Vec<String>,
}

/// Keep runaway scripts from freezing the client.
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

impl ScriptHost {
    pub fn new() -> Result<Self, String> {
        let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE;
        let lua = Lua::new_with(libs, LuaOptions::default()).map_err(|e| e.to_string())?;
        let _ = lua.set_memory_limit(MEMORY_LIMIT);
        let state = Rc::new(RefCell::new(State::default()));
        install_api(&lua, &state).map_err(|e| e.to_string())?;
        Ok(ScriptHost {
            lua,
            state,
            loaded: Vec::new(),
        })
    }

    /// Load every `*.lua` in `dir`, in name order. Returns one error string per failed script.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let mut errors = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return errors;
        };
        let mut files: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "lua"))
            .collect();
        files.sort();
        for path in files {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            match std::fs::read_to_string(&path) {
                Ok(src) => {
                    if let Err(e) = self.load(&name, &src) {
                        errors.push(e);
                    }
                }
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        errors
    }

    pub fn load(&mut self, name: &str, src: &str) -> Result<(), String> {
        self.state.borrow_mut().loading = Some(name.to_string());
        let r = self.lua.load(src).set_name(format!("@{name}.lua")).exec();
        self.state.borrow_mut().loading = None;
        r.map_err(|e| format!("{name}: {}", short(&e)))?;
        self.loaded.push(name.to_string());
        Ok(())
    }

    pub fn loaded(&self) -> &[String] {
        &self.loaded
    }

    /// Custom commands with their help text, sorted.
    pub fn commands(&self) -> Vec<(String, String)> {
        let mut v: Vec<_> = self
            .state
            .borrow()
            .commands
            .iter()
            .map(|(k, (_, h))| (k.clone(), h.clone()))
            .collect();
        v.sort();
        v
    }

    fn set_ctx(&self, ctx: &Ctx) {
        self.state.borrow_mut().ctx = ctx.clone();
    }

    fn hooks(&self, event: &str) -> Vec<Function> {
        self.state
            .borrow()
            .hooks
            .get(event)
            .cloned()
            .unwrap_or_default()
    }

    /// Run `message` hooks. Returns false if any hook asked to hide the line.
    pub fn on_message(&self, ctx: &Ctx, m: &Message) -> Result<bool, String> {
        let hooks = self.hooks("message");
        if hooks.is_empty() {
            return Ok(true);
        }
        self.set_ctx(ctx);
        let t = self.message_table(m).map_err(|e| short(&e))?;
        let mut keep = true;
        for f in hooks {
            match f.call::<Value>(t.clone()) {
                Ok(Value::Boolean(false)) => keep = false,
                Ok(_) => {}
                Err(e) => return Err(short(&e)),
            }
        }
        Ok(keep)
    }

    /// Run `event` hooks with a table of string fields (join, part, connect...).
    pub fn on_event(&self, ctx: &Ctx, event: &str, fields: &[(&str, &str)]) -> Result<(), String> {
        let hooks = self.hooks(event);
        if hooks.is_empty() {
            return Ok(());
        }
        self.set_ctx(ctx);
        let t = self.lua.create_table().map_err(|e| short(&e))?;
        for (k, v) in fields {
            t.set(*k, *v).map_err(|e| short(&e))?;
        }
        for f in hooks {
            f.call::<()>(t.clone()).map_err(|e| short(&e))?;
        }
        Ok(())
    }

    /// Run `input` hooks on text the user typed. `Ok(None)` cancels sending.
    pub fn on_input(&self, ctx: &Ctx, text: &str) -> Result<Option<String>, String> {
        let hooks = self.hooks("input");
        if hooks.is_empty() {
            return Ok(Some(text.to_string()));
        }
        self.set_ctx(ctx);
        let mut text = text.to_string();
        for f in hooks {
            match f.call::<Value>(text.clone()).map_err(|e| short(&e))? {
                Value::Boolean(false) => return Ok(None),
                Value::String(s) => text = s.to_str().map(|s| s.to_string()).unwrap_or(text),
                _ => {}
            }
        }
        Ok(Some(text))
    }

    /// Run a script command. `None` when no script defines `name`.
    pub fn run_command(&self, ctx: &Ctx, name: &str, args: &str) -> Option<Result<(), String>> {
        let f = self
            .state
            .borrow()
            .commands
            .get(&name.to_lowercase())
            .map(|(f, _)| f.clone())?;
        self.set_ctx(ctx);
        Some(f.call::<()>(args.to_string()).map_err(|e| short(&e)))
    }

    /// Fire due timers. Errors are returned; a failing timer is dropped.
    pub fn tick(&self, now: Instant) -> Vec<String> {
        let due: Vec<(u64, Function, Ctx)> = {
            let mut st = self.state.borrow_mut();
            let mut out = Vec::new();
            st.timers.retain_mut(|t| {
                if t.due > now {
                    return true;
                }
                out.push((t.id, t.func.clone(), t.ctx.clone()));
                match t.every {
                    Some(d) => {
                        t.due = now + d;
                        true
                    }
                    None => false,
                }
            });
            out
        };
        let mut errors = Vec::new();
        for (id, f, ctx) in due {
            self.set_ctx(&ctx);
            if let Err(e) = f.call::<()>(()) {
                errors.push(short(&e));
                self.state.borrow_mut().timers.retain(|t| t.id != id);
            }
        }
        errors
    }

    pub fn take_actions(&self) -> Vec<(Ctx, Action)> {
        std::mem::take(&mut self.state.borrow_mut().actions)
    }

    fn message_table(&self, m: &Message) -> mlua::Result<Table> {
        let t = self.lua.create_table()?;
        t.set("network", m.network.as_str())?;
        t.set("buffer", m.buffer.as_str())?;
        t.set("nick", m.nick.as_str())?;
        t.set("text", m.text.as_str())?;
        t.set("kind", m.kind)?;
        t.set("own", m.own)?;
        t.set("highlight", m.highlight)?;
        if let Some(id) = &m.msgid {
            t.set("msgid", id.as_str())?;
        }
        Ok(t)
    }
}

/// First line of a Lua error, without the traceback noise.
fn short(e: &mlua::Error) -> String {
    let s = e.to_string();
    s.lines().next().unwrap_or("").to_string()
}

fn install_api(lua: &Lua, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let murk = lua.create_table()?;
    murk.set("version", env!("CARGO_PKG_VERSION"))?;

    let st = state.clone();
    murk.set(
        "on",
        lua.create_function(move |_, (event, f): (String, Function)| {
            let known = ["message", "input", "join", "part", "connect", "disconnect"];
            if !known.contains(&event.as_str()) {
                return Err(mlua::Error::runtime(format!(
                    "unknown event '{event}', expected one of: {}",
                    known.join(", ")
                )));
            }
            st.borrow_mut().hooks.entry(event).or_default().push(f);
            Ok(())
        })?,
    )?;

    let st = state.clone();
    murk.set(
        "command",
        lua.create_function(
            move |_, (name, f, help): (String, Function, Option<String>)| {
                let name = name.trim_start_matches('/').to_lowercase();
                let help = help.unwrap_or_else(|| format!("/{name}"));
                st.borrow_mut().commands.insert(name, (f, help));
                Ok(())
            },
        )?,
    )?;

    // say(text) to the current buffer, or say(target, text).
    let st = state.clone();
    murk.set(
        "say",
        lua.create_function(move |_, (a, b): (String, Option<String>)| {
            let mut s = st.borrow_mut();
            let ctx = s.ctx.clone();
            let (target, text) = match b {
                Some(text) => (a, text),
                None => (
                    ctx.buffer.clone().ok_or_else(|| {
                        mlua::Error::runtime("say: no current buffer; pass a target")
                    })?,
                    a,
                ),
            };
            s.actions.push((ctx, Action::Say { target, text }));
            Ok(())
        })?,
    )?;

    let st = state.clone();
    murk.set(
        "me",
        lua.create_function(move |_, (a, b): (String, Option<String>)| {
            let mut s = st.borrow_mut();
            let ctx = s.ctx.clone();
            let (target, text) = match b {
                Some(text) => (a, text),
                None => (
                    ctx.buffer.clone().ok_or_else(|| {
                        mlua::Error::runtime("me: no current buffer; pass a target")
                    })?,
                    a,
                ),
            };
            s.actions.push((ctx, Action::Action { target, text }));
            Ok(())
        })?,
    )?;

    for (name, make) in [
        ("raw", Action::Raw as fn(String) -> Action),
        ("print", Action::Print as fn(String) -> Action),
        ("run", Action::Run as fn(String) -> Action),
    ] {
        let st = state.clone();
        murk.set(
            name,
            lua.create_function(move |_, text: String| {
                let mut s = st.borrow_mut();
                let ctx = s.ctx.clone();
                s.actions.push((ctx, make(text)));
                Ok(())
            })?,
        )?;
    }

    let st = state.clone();
    murk.set(
        "react",
        lua.create_function(move |_, (msgid, emoji): (String, String)| {
            let mut s = st.borrow_mut();
            let ctx = s.ctx.clone();
            s.actions.push((ctx, Action::React { msgid, emoji }));
            Ok(())
        })?,
    )?;

    // timer(seconds, fn[, repeat]) -> id
    let st = state.clone();
    murk.set(
        "timer",
        lua.create_function(move |_, (secs, f, repeat): (f64, Function, Option<bool>)| {
            if !(secs.is_finite() && secs >= 0.1) {
                return Err(mlua::Error::runtime(
                    "timer: interval must be at least 0.1 seconds",
                ));
            }
            let d = Duration::from_secs_f64(secs);
            let mut s = st.borrow_mut();
            s.next_timer += 1;
            let id = s.next_timer;
            let ctx = s.ctx.clone();
            s.timers.push(Timer {
                id,
                due: Instant::now() + d,
                every: repeat.unwrap_or(false).then_some(d),
                func: f,
                ctx,
            });
            Ok(id)
        })?,
    )?;

    let st = state.clone();
    murk.set(
        "cancel",
        lua.create_function(move |_, id: u64| {
            st.borrow_mut().timers.retain(|t| t.id != id);
            Ok(())
        })?,
    )?;

    lua.globals().set("murk", murk)?;

    // A safe slice of `os`: clocks and dates, nothing that touches the system.
    let os = lua.create_table()?;
    os.set(
        "time",
        lua.create_function(|_, ()| {
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0))
        })?,
    )?;
    let start = Instant::now();
    os.set(
        "clock",
        lua.create_function(move |_, ()| Ok(start.elapsed().as_secs_f64()))?,
    )?;
    lua.globals().set("os", os)?;

    // print() goes to the client rather than a console nobody sees.
    let st = state.clone();
    lua.globals().set(
        "print",
        lua.create_function(move |_, args: mlua::Variadic<Value>| {
            let text = args
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.to_string_lossy().to_string(),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>()
                .join("\t");
            let mut s = st.borrow_mut();
            let ctx = s.ctx.clone();
            s.actions.push((ctx, Action::Print(text)));
            Ok(())
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            network: 1,
            buffer: Some("#murk".into()),
        }
    }

    #[test]
    fn message_hooks_say_and_hide() {
        let mut h = ScriptHost::new().unwrap();
        h.load(
            "bot",
            r#"
            murk.on("message", function(m)
              if m.text == "!ping" then murk.say("pong, " .. m.nick) end
              if m.text:find("spoiler") then return false end
            end)
            "#,
        )
        .unwrap();
        let m = Message {
            nick: "alice".into(),
            text: "!ping".into(),
            kind: "msg",
            ..Default::default()
        };
        assert!(h.on_message(&ctx(), &m).unwrap());
        assert_eq!(
            h.take_actions(),
            vec![(
                ctx(),
                Action::Say {
                    target: "#murk".into(),
                    text: "pong, alice".into()
                }
            )]
        );
        let m = Message {
            text: "big spoiler".into(),
            ..m
        };
        assert!(!h.on_message(&ctx(), &m).unwrap());
    }

    #[test]
    fn commands_input_and_timers() {
        let mut h = ScriptHost::new().unwrap();
        h.load(
            "fun",
            r#"
            murk.command("slap", function(a) murk.me("slaps " .. a .. " with a trout") end, "/slap <nick>")
            murk.on("input", function(t) if t == "nope" then return false end return (t:gsub("teh", "the")) end)
            murk.timer(0.1, function() murk.print("tick") end)
            "#,
        )
        .unwrap();
        assert_eq!(h.commands(), vec![("slap".into(), "/slap <nick>".into())]);
        h.run_command(&ctx(), "SLAP", "bob").unwrap().unwrap();
        assert!(
            matches!(&h.take_actions()[0].1, Action::Action { text, .. } if text == "slaps bob with a trout")
        );
        assert_eq!(
            h.on_input(&ctx(), "teh end").unwrap().as_deref(),
            Some("the end")
        );
        assert_eq!(h.on_input(&ctx(), "nope").unwrap(), None);
        assert!(h.run_command(&ctx(), "missing", "").is_none());
        assert!(h.tick(Instant::now() + Duration::from_secs(1)).is_empty());
        assert_eq!(h.take_actions()[0].1, Action::Print("tick".into()));
        // One-shot timers do not fire twice.
        h.tick(Instant::now() + Duration::from_secs(2));
        assert!(h.take_actions().is_empty());
    }

    #[test]
    fn sandbox_has_no_io_or_exec() {
        let mut h = ScriptHost::new().unwrap();
        assert!(h.load("evil", "io.open('x')").is_err());
        assert!(h.load("evil2", "os.execute('calc')").is_err());
        assert!(h.load("evil3", "require('socket')").is_err());
        assert!(h.load("ok", "local t = os.time(); assert(t > 0)").is_ok());
        assert!(
            h.load("bad_event", "murk.on('nonsense', function() end)")
                .is_err()
        );
    }
}
