//! Client state and the glue between input, engine events and the log store.

use crate::editor::Editor;
use murk_config::Config;
use murk_core::input::{self, Action, CommandEq};
use murk_core::{Command, ConnState, Engine, Event, Line, LineKind, NetworkId};
use murk_store::Store;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime};

/// Lines kept in memory per buffer; older ones page back in from the store.
const BUFFER_CAP: usize = 5000;
const TYPING_SEND_INTERVAL: Duration = Duration::from_secs(3);
const TYPING_EXPIRE: Duration = Duration::from_secs(6);

/// Written to the scripts folder on first run. Rename to `.lua` to enable it.
const EXAMPLE_SCRIPT: &str = r#"-- Murk scripts are Lua 5.4. Every *.lua file in this folder loads at startup;
-- run /script reload after editing. Rename this file to example.lua to try it.
--
-- murk.on(event, fn)        events: message, input, join, part, connect, disconnect
-- murk.command(name, fn, help)   adds /name; fn receives the argument string
-- murk.say([target,] text)  murk.me([target,] text)  murk.raw(line)
-- murk.print(text)          shows text locally, sends nothing
-- murk.react(msgid, emoji)  murk.run("/join #x")
-- murk.timer(seconds, fn[, repeat]) returns an id; murk.cancel(id)
--
-- A message hook gets m.network, m.buffer, m.nick, m.text, m.kind, m.own,
-- m.highlight and m.msgid. Return false to hide the line.
-- An input hook gets the text you typed: return new text, or false to cancel.

murk.command("slap", function(who)
  murk.me("slaps " .. who .. " around a bit with a large trout")
end, "/slap <nick>")

murk.command("shrug", function(rest)
  murk.say((rest ~= "" and rest .. " " or "") .. "\u{AF}\\_(\u{30C4})_/\u{AF}")
end, "/shrug [text]")

murk.on("message", function(m)
  if not m.own and m.text:lower():match("^!roll") then
    murk.say(m.nick .. " rolled a " .. math.random(1, 20))
  end
end)
"#;

pub struct Buffer {
    pub network: NetworkId,
    /// Channel or query name; `None` for the network's server buffer.
    pub name: Option<String>,
    pub lines: VecDeque<Line>,
    pub unread: u32,
    pub highlighted: bool,
    /// Rows scrolled up from the bottom.
    pub scroll: usize,
    /// Store id of the oldest loaded line, for paging further back.
    pub oldest_id: Option<i64>,
    pub history_exhausted: bool,
    pub typing: HashMap<String, Instant>,
    pub joined: bool,
    /// Reactions by target msgid: emoji and who reacted, in arrival order.
    pub reactions: HashMap<String, Vec<(String, Vec<String>)>>,
    /// Time of the newest line seen before the user last left this buffer.
    pub read_marker: Option<SystemTime>,
    /// Bumped on every visible change, so views can skip identical redraws.
    pub rev: u64,
}

impl Buffer {
    fn new(network: NetworkId, name: Option<String>) -> Self {
        Buffer {
            network,
            name,
            lines: VecDeque::new(),
            unread: 0,
            highlighted: false,
            scroll: 0,
            oldest_id: None,
            history_exhausted: false,
            typing: HashMap::new(),
            joined: false,
            reactions: HashMap::new(),
            read_marker: None,
            rev: 0,
        }
    }

    /// Toggle-free add: a nick reacting twice with the same emoji counts once.
    pub fn add_reaction(&mut self, msgid: String, emoji: String, nick: String) {
        let list = self.reactions.entry(msgid).or_default();
        match list.iter_mut().find(|(e, _)| *e == emoji) {
            Some((_, nicks)) if nicks.contains(&nick) => return,
            Some((_, nicks)) => nicks.push(nick),
            None => list.push((emoji, vec![nick])),
        }
        self.rev += 1;
    }

    /// The line with this msgid, if still loaded.
    pub fn find_msgid(&self, msgid: &str) -> Option<&Line> {
        self.lines
            .iter()
            .rev()
            .find(|l| l.msgid.as_deref() == Some(msgid))
    }

    pub fn is_channel_like(&self) -> bool {
        self.name
            .as_deref()
            .is_some_and(|n| n.starts_with(['#', '&', '!', '+']))
    }
}

pub struct Network {
    pub name: String,
    pub state: ConnState,
    pub nick: String,
    pub detail: Option<String>,
}

pub struct App {
    pub engine: Engine,
    pub store: Option<Store>,
    pub config: Config,
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub editor: Editor,
    pub networks: HashMap<NetworkId, Network>,
    pub status: Option<(String, Instant)>,
    pub quit: bool,
    pub bell: bool,
    last_typing_sent: Option<Instant>,
    /// Protocol traffic log, enabled with MURK_RAW_LOG=<path>.
    raw_log: Option<std::fs::File>,
    /// Channels the user asked to join, focused when the join lands.
    pending_focus: Vec<(NetworkId, String)>,
    /// Lua scripts from the config directory's `scripts` folder.
    pub scripts: Option<murk_script::ScriptHost>,
    pub transfers: crate::dcc::Transfers,
    /// Where settings changed from commands (such as /theme) are saved.
    pub config_path: Option<std::path::PathBuf>,
    /// Networks we already explained do not relay reactions.
    told_no_reactions: Vec<NetworkId>,
}

impl App {
    pub fn new(engine: Engine, store: Option<Store>, config: Config) -> Self {
        let mut app = App::bare(engine, store, config);
        app.load_scripts();
        app
    }

    /// An App without scripts, for tests and embedders that manage scripting themselves.
    pub fn bare(engine: Engine, store: Option<Store>, config: Config) -> Self {
        App {
            scripts: None,
            transfers: Default::default(),
            config_path: None,
            told_no_reactions: Vec::new(),
            engine,
            store,
            config,
            buffers: Vec::new(),
            active: 0,
            editor: Editor::default(),
            networks: HashMap::new(),
            status: None,
            quit: false,
            bell: false,
            last_typing_sent: None,
            pending_focus: Vec::new(),
            raw_log: std::env::var_os("MURK_RAW_LOG").and_then(|p| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .ok()
            }),
        }
    }

    pub fn current(&self) -> Option<&Buffer> {
        self.buffers.get(self.active)
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), Instant::now()));
    }

    fn net_name(&self, id: NetworkId) -> String {
        self.networks
            .get(&id)
            .map(|n| n.name.clone())
            .unwrap_or_default()
    }

    /// Start a configured or ad-hoc network and create its server buffer.
    pub fn connect(&mut self, name: &str) -> Result<NetworkId, String> {
        if let Some((&id, _)) = self
            .networks
            .iter()
            .find(|(_, n)| n.name.eq_ignore_ascii_case(name))
        {
            self.engine.send(id, Command::Connect);
            return Ok(id);
        }
        let net = match self.config.network(name) {
            Some(n) => n.clone(),
            None if name.contains('.') => {
                // Ad-hoc "host[:port]"; a leading '+' on the port means TLS, as in other clients.
                let (host, port) = name.rsplit_once(':').unwrap_or((name, "+6697"));
                let tls = port.starts_with('+') || port == "6697";
                murk_config::Network {
                    name: host.to_string(),
                    host: host.to_string(),
                    port: port
                        .trim_start_matches('+')
                        .parse()
                        .map_err(|_| format!("bad port {port}"))?,
                    tls,
                    ..Default::default()
                }
            }
            None => {
                return Err(format!(
                    "No network named {name}; add it to the config or use /connect host:port"
                ));
            }
        };
        let mut cfg = self.config.engine_config(&net).map_err(|e| e.to_string())?;
        cfg.name = net.name.clone();
        let id = self.engine.add_network(cfg);
        self.networks.insert(
            id,
            Network {
                name: net.name.clone(),
                state: ConnState::Connecting,
                nick: net
                    .nick
                    .clone()
                    .unwrap_or_else(|| self.config.identity.nick.clone()),
                detail: None,
            },
        );
        let idx = self.buffer_index(id, None);
        self.active = idx;
        Ok(id)
    }

    /// Index of a buffer, creating it after the network's last buffer if missing.
    pub fn buffer_index(&mut self, network: NetworkId, name: Option<&str>) -> usize {
        let fold = |s: &str| s.to_lowercase();
        if let Some(i) = self
            .buffers
            .iter()
            .position(|b| b.network == network && b.name.as_deref().map(fold) == name.map(fold))
        {
            return i;
        }
        let mut buf = Buffer::new(network, name.map(str::to_string));
        self.load_history(&mut buf);
        let pos = match name {
            None => {
                // Server buffers go after every existing network's group.
                self.buffers.len()
            }
            Some(_) => self
                .buffers
                .iter()
                .rposition(|b| b.network == network)
                .map(|i| i + 1)
                .unwrap_or(self.buffers.len()),
        };
        self.buffers.insert(pos, buf);
        if pos <= self.active && self.buffers.len() > 1 {
            self.active += 1;
        }
        pos
    }

    fn load_history(&self, buf: &mut Buffer) {
        let Some(store) = &self.store else { return };
        let net = self.net_name(buf.network);
        let name = buf.name.as_deref().unwrap_or(murk_store::SERVER_BUFFER);
        let Ok(page) = store.page(&net, name, buf.oldest_id, self.config.ui.scrollback_page) else {
            return;
        };
        if (page.len() as u32) < self.config.ui.scrollback_page {
            buf.history_exhausted = true;
        }
        if let Some(first) = page.first() {
            buf.oldest_id = Some(first.id);
        }
        let ids: Vec<&str> = page.iter().filter_map(|s| s.msgid.as_deref()).collect();
        if let Ok(rs) = store.reactions(&net, name, &ids) {
            for (msgid, emoji, nick) in rs {
                buf.add_reaction(msgid, emoji, nick);
            }
        }
        for s in page.into_iter().rev() {
            buf.rev += 1;
            buf.lines.push_front(Line {
                network: buf.network,
                buffer: buf.name.clone(),
                kind: s.kind,
                sender: s.sender,
                text: s.text,
                time: s.time,
                msgid: s.msgid,
                own: s.own,
                highlight: s.highlight,
                history: true,
                reply_to: s.reply_to,
            });
        }
    }

    /// Page older lines into the active buffer when scrolled to the top.
    pub fn load_older(&mut self) {
        let Some(mut buf) =
            (self.active < self.buffers.len()).then(|| self.buffers.remove(self.active))
        else {
            return;
        };
        if !buf.history_exhausted {
            self.load_history(&mut buf);
        } else if let Some(name) = buf.name.clone() {
            let before = buf.lines.front().and_then(|l| l.msgid.clone());
            self.engine.send(
                buf.network,
                Command::History {
                    target: name,
                    before,
                    limit: 100,
                },
            );
        }
        self.buffers.insert(self.active, buf);
    }

    pub fn switch(&mut self, idx: usize) {
        if idx < self.buffers.len() {
            self.mark_read();
            self.active = idx;
            let b = &mut self.buffers[idx];
            b.unread = 0;
            b.highlighted = false;
        }
    }

    pub fn switch_relative(&mut self, delta: isize) {
        let n = self.buffers.len() as isize;
        if n > 0 {
            self.switch(((self.active as isize + delta).rem_euclid(n)) as usize);
        }
    }

    /// Jump to the next buffer with a highlight, else the next with unread lines.
    pub fn switch_activity(&mut self) {
        let n = self.buffers.len();
        let order = (1..n).map(|k| (self.active + k) % n);
        let target = order
            .clone()
            .find(|&i| self.buffers[i].highlighted)
            .or_else(|| order.clone().find(|&i| self.buffers[i].unread > 0));
        if let Some(i) = target {
            self.switch(i);
        }
    }

    fn mark_read(&mut self) {
        if let Some(b) = self.buffers.get_mut(self.active) {
            b.read_marker = b.lines.back().map(|l| l.time);
        }
        let Some(b) = self.buffers.get(self.active) else {
            return;
        };
        let (Some(store), Some(last)) = (&self.store, b.lines.back()) else {
            return;
        };
        let net = self.net_name(b.network);
        let name = b.name.as_deref().unwrap_or(murk_store::SERVER_BUFFER);
        let _ = store.set_read_marker(&net, name, last.time);
    }

    pub fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::State {
                network,
                state,
                detail,
            } => {
                if let Some(n) = self.networks.get_mut(&network) {
                    n.state = state;
                    n.detail = detail.clone();
                }
                let text = match (state, &detail) {
                    (ConnState::Connecting, _) => "Connecting...".to_string(),
                    (ConnState::Registering, _) => "Connected, registering".to_string(),
                    (ConnState::Connected, _) => "Registered".to_string(),
                    (ConnState::Disconnected, Some(d)) => format!("Disconnected: {d}"),
                    (ConnState::Disconnected, None) => "Disconnected".to_string(),
                };
                if state == ConnState::Disconnected {
                    for b in self.buffers.iter_mut().filter(|b| b.network == network) {
                        b.joined = false;
                    }
                }
                self.local_line(network, None, LineKind::Server, &text);
                let name = self.net_name(network);
                match state {
                    ConnState::Connected => {
                        self.script_event(network, "connect", &[("network", &name)])
                    }
                    ConnState::Disconnected => {
                        self.script_event(network, "disconnect", &[("network", &name)])
                    }
                    _ => {}
                }
            }
            Event::OwnNick { network, nick } => {
                if let Some(n) = self.networks.get_mut(&network) {
                    n.nick = nick;
                }
            }
            Event::Joined { network, channel } => {
                let on_server_buffer = self
                    .current()
                    .is_some_and(|b| b.network == network && b.name.is_none());
                let before = self.pending_focus.len();
                self.pending_focus
                    .retain(|(n, c)| !(*n == network && c.eq_ignore_ascii_case(&channel)));
                let requested = self.pending_focus.len() != before;
                let i = self.buffer_index(network, Some(&channel));
                self.buffers[i].joined = true;
                if requested || on_server_buffer {
                    self.switch(i);
                }
                let name = self.net_name(network);
                self.script_event(
                    network,
                    "join",
                    &[("network", &name), ("channel", &channel)],
                );
            }
            Event::Parted { network, channel } => {
                let i = self.buffer_index(network, Some(&channel));
                self.buffers[i].joined = false;
                let name = self.net_name(network);
                self.script_event(
                    network,
                    "part",
                    &[("network", &name), ("channel", &channel)],
                );
            }
            Event::Typing {
                network,
                buffer,
                nick,
                active,
            } => {
                // Servers echo our own typing notices back; we know when we are typing.
                if self
                    .networks
                    .get(&network)
                    .is_some_and(|n| n.nick.eq_ignore_ascii_case(&nick))
                {
                    return;
                }
                let i = self.buffer_index(network, Some(&buffer));
                if active {
                    self.buffers[i].typing.insert(nick, Instant::now());
                } else {
                    self.buffers[i].typing.remove(&nick);
                }
            }
            Event::Line(line) => self.add_line(line),
            Event::Raw {
                network,
                msg,
                outgoing,
            } => {
                if let Some(f) = &mut self.raw_log {
                    use std::io::Write;
                    let dir = if outgoing { ">>" } else { "<<" };
                    let _ = writeln!(f, "{network} {dir} {}", msg.to_wire());
                }
            }
            Event::React {
                network,
                buffer,
                msgid,
                nick,
                emoji,
            } => {
                if let Some(store) = &self.store {
                    let net = self.net_name(network);
                    let _ = store.add_reaction(&net, &buffer, &msgid, &nick, &emoji);
                }
                let i = self.buffer_index(network, Some(&buffer));
                self.buffers[i].add_reaction(msgid, emoji, nick);
            }
            Event::DccOffer {
                network,
                from,
                offer,
            } => {
                let size = offer.size.map(crate::dcc::human_size);
                match self.transfers.offered(network, &from, offer) {
                    Ok(id) => {
                        let t = self.transfers.list.iter().find(|t| t.id == id).unwrap();
                        let text = format!(
                            "{from} offers {} ({}). /dcc get {id} to download, /dcc decline {id} to refuse.",
                            t.filename,
                            size.unwrap_or_else(|| "size unknown".into())
                        );
                        let i = self.buffer_index(network, Some(&from));
                        self.local_line(network, Some(from), LineKind::Server, &text);
                        if i != self.active {
                            self.buffers[i].highlighted = true;
                            self.buffers[i].unread += 1;
                        }
                        self.bell = true;
                    }
                    Err(e) => self.local_line(network, Some(from), LineKind::Server, &e),
                }
            }
            Event::ChannelList { network, channels } => {
                // The desktop client has a browser; here the busiest matches are listed.
                let total = channels.len();
                let lines: Vec<String> = channels
                    .iter()
                    .take(40)
                    .map(|c| {
                        format!(
                            "{:>6}  {}  {}",
                            c.users,
                            c.name,
                            murk_proto::format::strip(&c.topic)
                        )
                    })
                    .collect();
                for l in lines {
                    self.local_line(network, None, LineKind::Server, &l);
                }
                let more = total.saturating_sub(40);
                if more > 0 {
                    self.local_line(
                        network,
                        None,
                        LineKind::Server,
                        &format!("...and {more} more. Narrow it with /list *word*"),
                    );
                }
            }
            // WHOIS already shows as server text; frontends with profile cards use the event.
            Event::Topic { .. } | Event::Members { .. } | Event::Whois { .. } => {}
        }
    }

    fn add_line(&mut self, mut line: Line) {
        let net = self.net_name(line.network);
        if let Some(sender) = &line.sender
            && self.config.ignore.iter().any(|m| mask_matches(m, sender))
        {
            return;
        }
        // Highlight words count like the nick, and are logged as highlights for the mentions view.
        if !line.highlight
            && !line.own
            && matches!(
                line.kind,
                LineKind::Privmsg | LineKind::Notice | LineKind::Action
            )
        {
            let lower = line.text.to_lowercase();
            line.highlight = self
                .config
                .ui
                .highlights
                .iter()
                .any(|w| !w.is_empty() && lower.contains(&w.to_lowercase()));
        }
        // Already logged means a replay of something we have seen.
        if let Some(store) = &self.store
            && let Ok(None) = store.insert(&net, &line)
        {
            return;
        }
        // Script hooks see live chat only: replayed history must not wake up bots.
        if !line.history
            && self.scripts.is_some()
            && matches!(
                line.kind,
                LineKind::Privmsg | LineKind::Notice | LineKind::Action
            )
        {
            let ctx = murk_script::Ctx {
                network: line.network,
                buffer: line.buffer.clone(),
            };
            let m = murk_script::Message {
                network: net.clone(),
                buffer: line.buffer.clone().unwrap_or_default(),
                nick: line.sender.clone().unwrap_or_default(),
                text: line.text.clone(),
                kind: match line.kind {
                    LineKind::Notice => "notice",
                    LineKind::Action => "action",
                    _ => "msg",
                },
                own: line.own,
                highlight: line.highlight,
                msgid: line.msgid.clone(),
            };
            let keep = match self.scripts.as_ref().map(|s| s.on_message(&ctx, &m)) {
                Some(Ok(k)) => k,
                Some(Err(e)) => {
                    self.set_status(format!("Script error in message hook: {e}"));
                    true
                }
                None => true,
            };
            self.run_script_actions();
            if !keep {
                return;
            }
        }
        let idx = self.buffer_index(line.network, line.buffer.as_deref());
        let is_active = idx == self.active;
        let counts = matches!(
            line.kind,
            LineKind::Privmsg | LineKind::Notice | LineKind::Action
        ) && !line.own;
        let is_query = line.buffer.is_some() && !self.buffers[idx].is_channel_like();
        // Muted buffers stay quiet unless someone addresses us directly.
        let muted = self.is_muted(idx) && !line.highlight;
        let buf = &mut self.buffers[idx];
        if let Some(s) = &line.sender {
            buf.typing.remove(s);
        }
        if !is_active && counts && !line.history && !muted {
            buf.unread += 1;
            if line.highlight || is_query {
                buf.highlighted = true;
                self.bell = true;
            }
        }
        if is_active && buf.scroll > 0 {
            // Keep the viewport steady while reading back.
            buf.scroll += 1;
        }
        buf.rev += 1;
        buf.lines.push_back(line);
        if buf.lines.len() > BUFFER_CAP {
            buf.lines.pop_front();
            buf.history_exhausted = false;
            buf.oldest_id = None;
        }
    }

    fn local_line(
        &mut self,
        network: NetworkId,
        buffer: Option<String>,
        kind: LineKind,
        text: &str,
    ) {
        let idx = self.buffer_index(network, buffer.as_deref());
        self.buffers[idx].rev += 1;
        self.buffers[idx].lines.push_back(Line {
            network,
            buffer,
            kind,
            sender: None,
            text: text.to_string(),
            time: SystemTime::now(),
            msgid: None,
            own: false,
            highlight: false,
            history: false,
            reply_to: None,
        });
    }

    fn info(&mut self, text: &str) {
        if let Some(b) = self.current() {
            let (net, name) = (b.network, b.name.clone());
            self.local_line(net, name, LineKind::Server, text);
        } else {
            self.set_status(text);
        }
    }

    /// `MURK_SCRIPTS_DIR` overrides the default `<config dir>/scripts`.
    pub fn scripts_dir() -> std::path::PathBuf {
        std::env::var_os("MURK_SCRIPTS_DIR")
            .map(Into::into)
            .unwrap_or_else(|| murk_config::config_dir().join("scripts"))
    }

    /// (Re)load every script. Failures are reported in the active buffer.
    pub fn load_scripts(&mut self) {
        let dir = Self::scripts_dir();
        if !dir.exists() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("example.lua.txt"), EXAMPLE_SCRIPT);
        }
        let mut host = match murk_script::ScriptHost::new() {
            Ok(h) => h,
            Err(e) => {
                self.set_status(format!("Scripting unavailable: {e}"));
                return;
            }
        };
        let errors = host.load_dir(&dir);
        self.scripts = Some(host);
        for e in errors {
            self.info(&format!("Script failed to load: {e}"));
        }
    }

    fn script_ctx(&self) -> murk_script::Ctx {
        match self.current() {
            Some(b) => murk_script::Ctx {
                network: b.network,
                buffer: b.name.clone(),
            },
            None => murk_script::Ctx::default(),
        }
    }

    /// Accept a DCC offer (the newest pending one when `id` is None).
    pub fn dcc_accept(&mut self, id: Option<u32>) {
        let id = id.or_else(|| {
            self.transfers
                .list
                .iter()
                .rev()
                .find(|t| t.state == crate::dcc::State::Offered)
                .map(|t| t.id)
        });
        let Some(id) = id else {
            return self.info("No pending DCC offers");
        };
        match self.transfers.accept(id, &self.engine) {
            Ok(p) => self.info(&format!("Downloading to {}", p.display())),
            Err(e) => self.info(&e),
        }
    }

    pub fn dcc_decline(&mut self, id: u32) {
        if !self.transfers.decline(id) {
            self.info("No such pending offer");
        }
    }

    /// Offer a file to a nick on the active buffer's network.
    pub fn dcc_send(&mut self, nick: &str, path: &str) {
        let Some(net) = self.current().map(|b| b.network) else {
            return self.info("Not connected");
        };
        let path = std::path::PathBuf::from(path.trim().trim_matches('"'));
        match self.transfers.send(net, nick, path, &self.engine) {
            Ok(_) => self.info(&format!(
                "Offered to {nick}. They need to reach this machine directly: on the same network, or with a forwarded port."
            )),
            Err(e) => self.info(&e),
        }
    }

    fn dcc_command(&mut self, args: &str) {
        let mut it = args.split_whitespace();
        match it.next().unwrap_or("list") {
            "get" | "accept" => self.dcc_accept(it.next().and_then(|s| s.parse().ok())),
            "decline" | "reject" => match it.next().and_then(|s| s.parse().ok()) {
                Some(id) => self.dcc_decline(id),
                None => self.info("Usage: /dcc decline <id>"),
            },
            "send" => {
                let nick = it.next().unwrap_or("").to_string();
                let path = args.splitn(3, ' ').nth(2).unwrap_or("").to_string();
                if nick.is_empty() || path.is_empty() {
                    return self.info("Usage: /dcc send <nick> <path>");
                }
                self.dcc_send(&nick, &path);
            }
            "list" => {
                if self.transfers.list.is_empty() {
                    return self.info("No DCC transfers. /dcc send <nick> <path> offers a file.");
                }
                let lines: Vec<String> = self
                    .transfers
                    .list
                    .iter()
                    .map(|t| {
                        let dir = if t.outgoing { "to" } else { "from" };
                        let size = t
                            .size
                            .map(crate::dcc::human_size)
                            .unwrap_or_else(|| "?".into());
                        let state = match &t.state {
                            crate::dcc::State::Offered => "offered: /dcc get to accept".to_string(),
                            crate::dcc::State::Waiting => "waiting for them to connect".into(),
                            crate::dcc::State::Active => format!(
                                "{} of {size}, {}/s",
                                crate::dcc::human_size(t.done),
                                crate::dcc::human_size(t.rate as u64)
                            ),
                            crate::dcc::State::Done => "done".into(),
                            crate::dcc::State::Failed(e) => format!("failed: {e}"),
                            crate::dcc::State::Declined => "declined".into(),
                        };
                        format!(
                            "  #{} {} {dir} {} ({size}): {state}",
                            t.id, t.filename, t.peer
                        )
                    })
                    .collect();
                for l in lines {
                    self.info(&l);
                }
            }
            other => self.info(&format!(
                "Unknown /dcc action '{other}': use list, get, decline or send"
            )),
        }
    }

    fn script_command(&mut self, args: &str) {
        match args {
            "reload" => {
                self.load_scripts();
                let n = self.scripts.as_ref().map_or(0, |s| s.loaded().len());
                self.info(&format!(
                    "Reloaded {n} script(s) from {}",
                    Self::scripts_dir().display()
                ));
            }
            "" | "list" => {
                let (loaded, cmds) = match &self.scripts {
                    Some(s) => (s.loaded().join(", "), s.commands()),
                    None => (String::new(), vec![]),
                };
                self.info(&format!(
                    "Scripts in {}: {}",
                    Self::scripts_dir().display(),
                    if loaded.is_empty() { "none" } else { &loaded }
                ));
                for (_, help) in cmds {
                    self.info(&format!("  {help}"));
                }
                self.info("/script reload picks up changes. See example.lua.txt in that folder.");
            }
            other => self.info(&format!(
                "Unknown /script action '{other}': use list or reload"
            )),
        }
    }

    /// Carry out what scripts asked for. Runs a few rounds so scripts reacting to
    /// their own output settle, without letting a loop run away.
    fn run_script_actions(&mut self) {
        for _ in 0..4 {
            let Some(actions) = self.scripts.as_ref().map(|s| s.take_actions()) else {
                return;
            };
            if actions.is_empty() {
                return;
            }
            for (ctx, a) in actions {
                let net = ctx.network;
                use murk_script::Action as A;
                match a {
                    A::Say { target, text } => {
                        self.engine.send(
                            net,
                            Command::Say {
                                target,
                                text,
                                action: false,
                            },
                        );
                    }
                    A::Action { target, text } => {
                        self.engine.send(
                            net,
                            Command::Say {
                                target,
                                text,
                                action: true,
                            },
                        );
                    }
                    A::Raw(line) => {
                        self.engine.send(net, Command::Raw(line));
                    }
                    A::Print(text) => {
                        if self.networks.contains_key(&net) {
                            self.local_line(net, ctx.buffer, LineKind::Server, &text);
                        } else {
                            self.info(&text);
                        }
                    }
                    A::React { msgid, emoji } => {
                        if let Some(target) = ctx.buffer {
                            self.engine.send(
                                net,
                                Command::React {
                                    target,
                                    msgid,
                                    emoji,
                                },
                            );
                        }
                    }
                    A::Run(line) => self.run_line(&line),
                }
            }
        }
    }

    fn script_event(&mut self, network: NetworkId, event: &str, fields: &[(&str, &str)]) {
        let ctx = murk_script::Ctx {
            network,
            buffer: fields
                .iter()
                .find(|(k, _)| *k == "channel")
                .map(|(_, v)| v.to_string()),
        };
        if let Some(s) = &self.scripts
            && let Err(e) = s.on_event(&ctx, event, fields)
        {
            self.info(&format!("Script error in {event} hook: {e}"));
        }
        self.run_script_actions();
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        for (_, msg) in self.transfers.poll() {
            self.info(&msg);
        }
        if let Some(s) = &self.scripts {
            for e in s.tick(now) {
                self.set_status(format!("Script timer failed: {e}"));
            }
            self.run_script_actions();
        }
        for b in &mut self.buffers {
            b.typing
                .retain(|_, t| now.duration_since(*t) < TYPING_EXPIRE);
        }
        if self
            .status
            .as_ref()
            .is_some_and(|(_, t)| now.duration_since(*t) > Duration::from_secs(5))
        {
            self.status = None;
        }
    }

    /// Called after each edit, to send throttled typing notifications.
    pub fn typing_changed(&mut self) {
        let Some(b) = self.current() else { return };
        let Some(target) = b.name.clone() else { return };
        let net = b.network;
        let text = self.editor.text();
        if text.is_empty() || text.starts_with('/') {
            return;
        }
        if self
            .last_typing_sent
            .is_some_and(|t| t.elapsed() < TYPING_SEND_INTERVAL)
        {
            return;
        }
        self.last_typing_sent = Some(Instant::now());
        self.engine.send(
            net,
            Command::Typing {
                target,
                active: true,
            },
        );
    }

    /// Key used for per-buffer settings such as muting: "Network/#chan" or "Network".
    pub fn buffer_key(&self, idx: usize) -> String {
        let Some(b) = self.buffers.get(idx) else {
            return String::new();
        };
        let net = self.net_name(b.network);
        match &b.name {
            Some(n) => format!("{net}/{}", n.to_lowercase()),
            None => net,
        }
    }

    pub fn is_muted(&self, idx: usize) -> bool {
        let key = self.buffer_key(idx);
        self.config
            .ui
            .muted
            .iter()
            .any(|m| m.eq_ignore_ascii_case(&key))
    }

    /// Mute or unmute a buffer. The caller saves the config.
    pub fn set_muted(&mut self, idx: usize, muted: bool) {
        let key = self.buffer_key(idx);
        self.config
            .ui
            .muted
            .retain(|m| !m.eq_ignore_ascii_case(&key));
        if muted {
            self.config.ui.muted.push(key);
            if let Some(b) = self.buffers.get_mut(idx) {
                b.unread = 0;
                b.highlighted = false;
            }
        }
    }

    pub fn mark_read_at(&mut self, idx: usize) {
        if let Some(b) = self.buffers.get_mut(idx) {
            b.unread = 0;
            b.highlighted = false;
            b.read_marker = b.lines.back().map(|l| l.time);
        }
    }

    /// Switch to a buffer named the way the store names it. False if the network is not running.
    pub fn open_stored(&mut self, network: &str, buffer: &str) -> bool {
        let Some(net) = self
            .networks
            .iter()
            .find(|(_, n)| n.name.eq_ignore_ascii_case(network))
            .map(|(id, _)| *id)
        else {
            return false;
        };
        let name = (buffer != murk_store::SERVER_BUFFER).then_some(buffer);
        let i = self.buffer_index(net, name);
        self.switch(i);
        true
    }

    /// Whether the network relays a client-only tag to other users.
    fn relays_tag(&self, net: NetworkId, tag: &str) -> bool {
        self.engine
            .with_state(net, |s| {
                s.has_cap("message-tags") && s.isupport.client_tag_allowed(tag)
            })
            .unwrap_or(false)
    }

    /// React to a message in the active buffer.
    pub fn react(&mut self, msgid: &str, emoji: &str) {
        let Some(b) = self.current() else { return };
        let Some(target) = b.name.clone() else { return };
        let net = b.network;
        if !self.relays_tag(net, "draft/react") && !self.told_no_reactions.contains(&net) {
            self.told_no_reactions.push(net);
            self.info(&format!(
                "{} does not pass reactions between people, so only you will see them here.",
                self.net_name(net)
            ));
        }
        self.engine.send(
            net,
            Command::React {
                target,
                msgid: msgid.to_string(),
                emoji: emoji.to_string(),
            },
        );
    }

    /// Send `text` in the active buffer as a reply to `msgid`. Commands are run normally.
    pub fn reply(&mut self, msgid: &str, text: &str) {
        if text.starts_with('/') {
            self.submit_text(text);
            return;
        }
        let Some(b) = self.current() else { return };
        let Some(target) = b.name.clone() else { return };
        let net = b.network;
        // Without reply tags, fall back to the classic "nick: text" so it still reads as a reply.
        if !self.relays_tag(net, "draft/reply") {
            let who = b.find_msgid(msgid).and_then(|l| l.sender.clone());
            let text = crate::emoji::expand(text);
            let text = match who {
                Some(n) if !text.starts_with(&format!("{n}:")) => format!("{n}: {text}"),
                _ => text,
            };
            self.engine.send(
                net,
                Command::Say {
                    target,
                    text,
                    action: false,
                },
            );
            return;
        }
        self.engine.send(
            net,
            Command::Reply {
                target,
                msgid: msgid.to_string(),
                text: crate::emoji::expand(text),
            },
        );
    }

    /// Nick and command completion candidates for `prefix`.
    pub fn completions(&self, prefix: &str) -> Vec<String> {
        let lower = prefix.to_lowercase();
        if let Some(cmd) = prefix.strip_prefix('/') {
            let cmd = cmd.to_lowercase();
            let scripted = self
                .scripts
                .as_ref()
                .map(|s| s.commands())
                .unwrap_or_default();
            let mut names: Vec<String> = input::HELP
                .iter()
                .map(|(n, _)| n.to_string())
                .chain(scripted.into_iter().map(|(n, _)| n))
                .chain(["script".to_string()])
                .filter(|n| n.starts_with(&cmd))
                .map(|n| format!("/{n}"))
                .collect();
            names.sort();
            names.dedup();
            return names;
        }
        if let Some(code) = prefix.strip_prefix(':')
            && !code.is_empty()
        {
            return crate::emoji::complete(&code.to_lowercase());
        }
        let Some(b) = self.current() else {
            return Vec::new();
        };
        if prefix.starts_with('#') {
            return self
                .buffers
                .iter()
                .filter(|x| x.network == b.network)
                .filter_map(|x| x.name.clone())
                .filter(|n| n.to_lowercase().starts_with(&lower))
                .collect();
        }
        let Some(name) = &b.name else {
            return Vec::new();
        };
        // Recent speakers first, then everyone else alphabetically.
        let mut out: Vec<String> = Vec::new();
        for l in b.lines.iter().rev().take(300) {
            if let Some(s) = &l.sender
                && s.to_lowercase().starts_with(&lower)
                && !out.contains(s)
                && !l.own
            {
                out.push(s.clone());
            }
        }
        let members = self
            .engine
            .with_state(b.network, |s| s.sorted_members(name))
            .unwrap_or_default();
        let mut rest: Vec<String> = members
            .into_iter()
            .map(|m| m.nick)
            .filter(|n| n.to_lowercase().starts_with(&lower) && !out.contains(n))
            .collect();
        rest.sort_by_key(|n| n.to_lowercase());
        out.extend(rest);
        if !b.is_channel_like() && name.to_lowercase().starts_with(&lower) && !out.contains(name) {
            out.push(name.clone());
        }
        out
    }

    pub fn submit_quit(&mut self) {
        self.run_line("/quit");
    }

    pub fn submit(&mut self) {
        let text = self.editor.take();
        self.submit_text(&text);
    }

    /// Run text one command or message per line, bypassing the editor and its history.
    pub fn submit_text(&mut self, text: &str) {
        for line in text.split('\n') {
            self.run_line(line);
        }
    }

    fn run_line(&mut self, text: &str) {
        // Aliases expand before parsing, one level deep.
        let expanded;
        let mut text = text;
        if let Some(body) = text.strip_prefix('/') {
            let (cmd, args) = body.split_once(' ').unwrap_or((body, ""));
            if let Some(alias) = self.config.aliases.get(&cmd.to_lowercase()) {
                expanded = murk_config::expand_alias(alias, args);
                text = &expanded;
            }
        }
        // Raw protocol lines go out verbatim; everything else gets :shortcode: emoji.
        let lower = text.to_ascii_lowercase();
        let emojied;
        if !(lower.starts_with("/quote ") || lower.starts_with("/raw ")) {
            emojied = crate::emoji::expand(text);
            text = &emojied;
        }

        // Scripts: /script itself, then script-defined commands, then the input filter.
        let ctx = self.script_ctx();
        let filtered;
        if let Some(body) = text.strip_prefix('/') {
            let (cmd, args) = body.split_once(' ').unwrap_or((body, ""));
            let cmd = cmd.to_lowercase();
            if cmd == "script" || cmd == "scripts" {
                self.script_command(args.trim());
                return;
            }
            if cmd == "theme" {
                const THEMES: [&str; 7] = [
                    "murk",
                    "aurora",
                    "ember",
                    "synthwave",
                    "mocha",
                    "phosphor",
                    "paper",
                ];
                let want = args.trim().to_lowercase();
                if THEMES.contains(&want.as_str()) {
                    self.config.ui.theme = want.clone();
                    if let Some(p) = &self.config_path {
                        let _ = self.config.save(p);
                    }
                    self.info(&format!("Theme set to {want}"));
                } else {
                    self.info(&format!(
                        "Themes: {}. Current: {}",
                        THEMES.join(", "),
                        self.config.ui.theme
                    ));
                }
                return;
            }
            if cmd == "dcc" {
                self.dcc_command(args.trim());
                return;
            }
            if cmd == "react" || cmd == "reply" {
                let last = self.current().and_then(|b| {
                    b.lines
                        .iter()
                        .rev()
                        .find(|l| {
                            !l.own
                                && l.msgid.is_some()
                                && matches!(l.kind, LineKind::Privmsg | LineKind::Action)
                        })
                        .and_then(|l| l.msgid.clone())
                });
                let args = args.trim();
                match (last, args.is_empty()) {
                    (None, _) => self.info("Nothing to respond to here yet"),
                    (Some(_), true) => self.info(&format!(
                        "Usage: /{cmd} <{}>",
                        if cmd == "react" { "emoji" } else { "text" }
                    )),
                    (Some(id), false) if cmd == "react" => {
                        let emoji = crate::emoji::lookup(args.trim_matches(':'))
                            .unwrap_or(args)
                            .to_string();
                        self.react(&id, &emoji);
                    }
                    (Some(id), false) => self.reply(&id, args),
                }
                return;
            }
            if !input::HELP.iter().any(|(n, _)| *n == cmd)
                && let Some(r) = self
                    .scripts
                    .as_ref()
                    .and_then(|s| s.run_command(&ctx, &cmd, args))
            {
                if let Err(e) = r {
                    self.info(&format!("Script error: {e}"));
                }
                self.run_script_actions();
                return;
            }
        } else if let Some(s) = &self.scripts {
            match s.on_input(&ctx, text) {
                Ok(Some(t)) => {
                    filtered = t;
                    text = &filtered;
                }
                Ok(None) => {
                    self.run_script_actions();
                    return;
                }
                Err(e) => self.info(&format!("Script error in input hook: {e}")),
            }
            self.run_script_actions();
        }

        let (net, target) = match self.current() {
            Some(b) => (Some(b.network), b.name.clone()),
            None => (None, None),
        };
        let is_channel = |s: &str| {
            net.and_then(|n| self.engine.with_state(n, |st| st.isupport.is_channel(s)))
                .unwrap_or_else(|| s.starts_with('#'))
        };
        let action = input::parse(text, target.as_deref(), is_channel);

        match action {
            Action::Nothing => {}
            Action::Error(e) => self.info(&e),
            Action::Send(CommandEq(cmd)) => match net {
                Some(n) => {
                    if let Command::Say { text, .. } = &cmd
                        && text.is_empty()
                    {
                        return;
                    }
                    // Only retract a typing notice we actually sent.
                    if let Some(name) = target.clone()
                        && matches!(cmd, Command::Say { .. })
                        && self.last_typing_sent.is_some()
                    {
                        self.engine.send(
                            n,
                            Command::Typing {
                                target: name,
                                active: false,
                            },
                        );
                        self.last_typing_sent = None;
                    }
                    if let Command::Join { channel, .. } = &cmd {
                        for c in channel.split(',') {
                            self.pending_focus.push((n, c.to_string()));
                        }
                    }
                    self.engine.send(n, cmd);
                }
                None => self.info("Not connected. Use /connect <network>"),
            },
            Action::Query { nick, text } => {
                let Some(n) = net else {
                    return self.info("Not connected");
                };
                let i = self.buffer_index(n, Some(&nick));
                self.switch(i);
                if let Some(t) = text {
                    self.engine.send(
                        n,
                        Command::Say {
                            target: nick,
                            text: t,
                            action: false,
                        },
                    );
                }
            }
            Action::Close => self.close_current(),
            Action::Connect(None) => match net {
                Some(n)
                    if self
                        .networks
                        .get(&n)
                        .is_some_and(|x| x.state == ConnState::Disconnected) =>
                {
                    self.engine.send(n, Command::Connect);
                }
                _ => {
                    let names: Vec<String> = self
                        .config
                        .networks
                        .iter()
                        .map(|n| n.name.clone())
                        .collect();
                    self.info(&format!(
                        "Networks: {}. Use /connect <name> or /connect host:port",
                        names.join(", ")
                    ));
                }
            },
            Action::Connect(Some(name)) => {
                if let Err(e) = self.connect(&name) {
                    self.info(&e);
                }
            }
            Action::Disconnect(reason) => {
                if let Some(n) = net {
                    let reason = reason.or_else(|| Some(self.config.identity.quit_message.clone()));
                    self.engine.send(n, Command::Disconnect { reason });
                }
            }
            Action::Search(q) => self.search(&q),
            Action::Clear => {
                if let Some(b) = self.buffers.get_mut(self.active) {
                    b.lines.clear();
                    b.rev += 1;
                    b.scroll = 0;
                }
            }
            Action::Help(topic) => {
                let lines: Vec<String> = match topic {
                    Some(t) => input::HELP
                        .iter()
                        .filter(|(n, _)| *n == t.trim_start_matches('/'))
                        .map(|(_, u)| u.to_string())
                        .collect(),
                    None => {
                        let mut v: Vec<String> =
                            input::HELP.iter().map(|(_, u)| u.to_string()).collect();
                        v.push("Keys: Alt+1..9 or Ctrl+P/N switch, Alt+A next activity, PgUp/PgDn scroll, Tab complete, Ctrl+C quit".into());
                        v
                    }
                };
                for l in lines {
                    self.info(&l);
                }
            }
            Action::Quit(reason) => {
                let reason = reason.unwrap_or_else(|| self.config.identity.quit_message.clone());
                for id in self.networks.keys() {
                    self.engine.send(
                        *id,
                        Command::Disconnect {
                            reason: Some(reason.clone()),
                        },
                    );
                }
                self.mark_read();
                self.quit = true;
            }
            Action::Window(w) => {
                if let Ok(n) = w.parse::<usize>() {
                    self.switch(n.saturating_sub(1));
                } else if let Some(i) = self.buffers.iter().position(|b| {
                    b.name
                        .as_deref()
                        .is_some_and(|x| x.eq_ignore_ascii_case(&w))
                }) {
                    self.switch(i);
                }
            }
        }
    }

    fn close_current(&mut self) {
        let Some(b) = self.current() else { return };
        let (net, name, joined, channel) =
            (b.network, b.name.clone(), b.joined, b.is_channel_like());
        match name {
            Some(n) => {
                if joined && channel {
                    self.engine.send(
                        net,
                        Command::Part {
                            channel: n,
                            reason: None,
                        },
                    );
                }
                self.buffers.remove(self.active);
            }
            None => {
                // Closing a server buffer removes the network and all its buffers.
                self.engine.remove_network(net);
                self.networks.remove(&net);
                self.buffers.retain(|b| b.network != net);
            }
        }
        self.active = self.active.min(self.buffers.len().saturating_sub(1));
    }

    fn search(&mut self, q: &str) {
        let Some(store) = &self.store else {
            return self.info("Logging is disabled");
        };
        let net = self.current().map(|b| self.net_name(b.network));
        let results = match store.search(q, net.as_deref(), None, 50) {
            Ok(r) => r,
            Err(e) => return self.info(&format!("Search failed: {e}")),
        };
        let mut out = vec![format!("{} result(s) for \"{q}\"", results.len())];
        for r in results.iter().rev() {
            out.push(format!(
                "[{}] {} <{}> {}",
                murk_core::time::format(r.time).get(..16).unwrap_or(""),
                r.buffer,
                r.sender.as_deref().unwrap_or("*"),
                r.text
            ));
        }
        for l in out {
            self.info(&l);
        }
    }
}

/// Glob match (`*` and `?`) of an ignore mask against a nick or full hostmask, case-insensitively.
pub fn mask_matches(mask: &str, nick: &str) -> bool {
    fn glob(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => glob(&p[1..], s) || (!s.is_empty() && glob(p, &s[1..])),
            (Some(b'?'), Some(_)) => glob(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => glob(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let target = if mask.contains('!') {
        nick.to_string()
    } else {
        nick.split('!').next().unwrap_or("").to_string()
    };
    glob(mask.as_bytes(), target.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        assert!(mask_matches("spam*", "SpamBot"));
        assert!(mask_matches("b?b", "bob"));
        assert!(!mask_matches("bob", "bobby"));
    }
}
