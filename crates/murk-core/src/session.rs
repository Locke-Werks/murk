//! Protocol state machine for one network. No I/O: messages in, lines and events out.

use crate::config::{NetworkConfig, Sasl};
use crate::event::{Command, ConnState, Event, Line, LineKind, NetworkId};
use crate::state::{Channel, Member, NetworkState};
use base64::Engine as _;
use murk_proto::{Message, Prefix, ctcp};
use std::collections::HashMap;
use std::time::SystemTime;

/// Capabilities we request when the server offers them.
pub const WANTED_CAPS: &[&str] = &[
    "account-notify",
    "account-tag",
    "away-notify",
    "batch",
    "cap-notify",
    "chghost",
    "draft/chathistory",
    "chathistory",
    "echo-message",
    "extended-join",
    "invite-notify",
    "labeled-response",
    "message-tags",
    "multi-prefix",
    "server-time",
    "setname",
    "userhost-in-names",
    "draft/read-marker",
    "standard-replies",
    "sasl",
];

/// SASL payloads are sent in chunks of at most this many bytes.
const SASL_CHUNK: usize = 400;

#[derive(Debug, Default)]
pub struct Output {
    pub send: Vec<Message>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CapPhase {
    Listing,
    Requesting(usize),
    Authenticating,
    Done,
}

#[derive(Debug)]
pub struct Session {
    pub id: NetworkId,
    pub config: NetworkConfig,
    pub state: NetworkState,
    cap_phase: CapPhase,
    offered: HashMap<String, Option<String>>,
    nick_attempt: usize,
    /// Open batches by reference tag: (type, params).
    batches: HashMap<String, (String, Vec<String>)>,
    pub highlights: Vec<String>,
    /// WHOIS replies being collected, by lowercased nick.
    whois: HashMap<String, crate::event::Whois>,
    /// LIST replies gathered until RPL_LISTEND.
    listing: Vec<crate::event::ListedChannel>,
}

impl Session {
    pub fn new(id: NetworkId, config: NetworkConfig) -> Self {
        let state = NetworkState {
            nick: config.nick.clone(),
            ..Default::default()
        };
        Session {
            id,
            config,
            state,
            cap_phase: CapPhase::Listing,
            offered: HashMap::new(),
            nick_attempt: 0,
            batches: HashMap::new(),
            whois: HashMap::new(),
            listing: Vec::new(),
            highlights: Vec::new(),
        }
    }

    /// Lines to send immediately after the transport connects.
    pub fn start(&mut self) -> Output {
        let nick = self.config.nick.clone();
        self.state = NetworkState {
            nick: nick.clone(),
            ..Default::default()
        };
        self.cap_phase = CapPhase::Listing;
        self.offered.clear();
        self.batches.clear();
        self.nick_attempt = 0;

        let mut out = Output::default();
        out.send.push(msg("CAP", &["LS", "302"]));
        if let Some(pass) = &self.config.password {
            out.send.push(msg("PASS", &[pass]));
        }
        out.send.push(msg("NICK", &[&nick]));
        out.send.push(msg(
            "USER",
            &[&self.config.username, "0", "*", &self.config.realname],
        ));
        out.events.push(Event::State {
            network: self.id,
            state: ConnState::Registering,
            detail: None,
        });
        out
    }

    fn line(
        &self,
        buffer: Option<String>,
        kind: LineKind,
        sender: Option<&str>,
        text: &str,
        m: &Message,
    ) -> Line {
        let time = m
            .tag("time")
            .and_then(crate::time::parse)
            .unwrap_or_else(SystemTime::now);
        let own = sender.is_some_and(|s| self.state.is_me(s));
        let highlight =
            !own && matches!(
                kind,
                LineKind::Privmsg | LineKind::Notice | LineKind::Action
            ) && self.is_highlight(text);
        let history = m
            .tag("batch")
            .and_then(|b| self.batches.get(b))
            .is_some_and(|(t, _)| t.ends_with("chathistory"));
        Line {
            network: self.id,
            buffer,
            kind,
            sender: sender.map(str::to_string),
            text: text.to_string(),
            time,
            msgid: m.tag("msgid").map(str::to_string),
            own,
            highlight,
            history,
            reply_to: m
                .tag("+draft/reply")
                .or_else(|| m.tag("+reply"))
                .map(str::to_string),
        }
    }

    fn is_highlight(&self, text: &str) -> bool {
        let plain = murk_proto::format::strip(text);
        let folded = self.state.fold(&plain);
        std::iter::once(self.state.nick.as_str())
            .chain(self.highlights.iter().map(String::as_str))
            .filter(|w| !w.is_empty())
            .any(|w| contains_word(&folded, &self.state.fold(w)))
    }

    fn server_line(&self, out: &mut Output, m: &Message, text: String) {
        let l = self.line(None, LineKind::Server, m.source_nick(), &text, m);
        out.events.push(Event::Line(l));
    }

    pub fn handle(&mut self, m: &Message) -> Output {
        let mut out = Output::default();
        out.events.push(Event::Raw {
            network: self.id,
            msg: m.clone(),
            outgoing: false,
        });
        let p = |i: usize| m.param(i).unwrap_or("");
        let src = m.source_nick().unwrap_or("").to_string();

        match m.command.as_str() {
            "PING" => out.send.push(Message::new("PONG", m.params.clone())),
            "CAP" => self.on_cap(m, &mut out),
            "AUTHENTICATE" => self.on_authenticate(m, &mut out),
            "900" => self.server_line(&mut out, m, m.params.last().cloned().unwrap_or_default()),
            "903" | "904" | "905" | "906" | "907" | "908" => {
                if m.command != "903" {
                    self.server_line(
                        &mut out,
                        m,
                        format!("SASL failed: {}", p(m.params.len().saturating_sub(1))),
                    );
                }
                if self.cap_phase == CapPhase::Authenticating {
                    self.end_cap(&mut out);
                }
            }
            "001" => {
                self.state.nick = p(0).to_string();
                self.state.registered = true;
                self.cap_phase = CapPhase::Done;
                out.events.push(Event::OwnNick {
                    network: self.id,
                    nick: self.state.nick.clone(),
                });
                out.events.push(Event::State {
                    network: self.id,
                    state: ConnState::Connected,
                    detail: None,
                });
                for line in &self.config.perform {
                    if let Ok(pm) = Message::parse(line) {
                        out.send.push(pm);
                    }
                }
                if !self.config.autojoin.is_empty() {
                    out.send
                        .push(msg("JOIN", &[&self.config.autojoin.join(",")]));
                }
                self.server_line(&mut out, m, p(1).to_string());
            }
            "005" => {
                let n = m.params.len();
                if n > 2 {
                    self.state
                        .isupport
                        .apply(m.params[1..n - 1].iter().map(String::as_str));
                }
            }
            "433" | "432" if !self.state.registered => {
                self.nick_attempt += 1;
                let next = self
                    .config
                    .alt_nicks
                    .get(self.nick_attempt - 1)
                    .cloned()
                    .unwrap_or_else(|| {
                        format!("{}{}", self.config.nick, "_".repeat(self.nick_attempt))
                    });
                self.state.nick = next.clone();
                out.send.push(msg("NICK", &[&next]));
            }
            "NICK" => {
                let new = p(0).to_string();
                let was_me = self.state.is_me(&src);
                let old_key = self.state.fold(&src);
                let new_key = self.state.fold(&new);
                let mut affected = Vec::new();
                for ch in self.state.channels.values_mut() {
                    if let Some(mut mem) = ch.members.remove(&old_key) {
                        mem.nick = new.clone();
                        ch.members.insert(new_key.clone(), mem);
                        affected.push(ch.name.clone());
                    }
                }
                if was_me {
                    self.state.nick = new.clone();
                    out.events.push(Event::OwnNick {
                        network: self.id,
                        nick: new.clone(),
                    });
                }
                for ch in affected {
                    let l = self.line(
                        Some(ch.clone()),
                        LineKind::Nick { new: new.clone() },
                        Some(&src),
                        "",
                        m,
                    );
                    out.events.push(Event::Line(l));
                    out.events.push(Event::Members {
                        network: self.id,
                        channel: ch,
                    });
                }
            }
            "JOIN" => {
                let chan = p(0).to_string();
                let (user, host) = user_host(m);
                if self.state.is_me(&src) {
                    let key = self.state.fold(&chan);
                    self.state.channels.insert(
                        key,
                        Channel {
                            name: chan.clone(),
                            ..Default::default()
                        },
                    );
                    out.events.push(Event::Joined {
                        network: self.id,
                        channel: chan.clone(),
                    });
                    if self.state.has_cap("chathistory") || self.state.has_cap("draft/chathistory")
                    {
                        out.send
                            .push(msg("CHATHISTORY", &["LATEST", &chan, "*", "50"]));
                    }
                }
                let account = if self.state.has_cap("extended-join") {
                    m.param(1).filter(|a| *a != "*").map(str::to_string)
                } else {
                    m.tag("account").map(str::to_string)
                };
                let key = self.state.fold(&src);
                if let Some(ch) = self.state.channel_mut(&chan) {
                    ch.members.insert(
                        key,
                        Member {
                            nick: src.clone(),
                            user,
                            host,
                            account,
                            ..Default::default()
                        },
                    );
                }
                let l = self.line(Some(chan.clone()), LineKind::Join, Some(&src), "", m);
                out.events.push(Event::Line(l));
                out.events.push(Event::Members {
                    network: self.id,
                    channel: chan,
                });
            }
            "PART" => {
                let chan = p(0).to_string();
                let reason = m.param(1).map(str::to_string);
                let l = self.line(
                    Some(chan.clone()),
                    LineKind::Part { reason },
                    Some(&src),
                    "",
                    m,
                );
                out.events.push(Event::Line(l));
                self.remove_member(&chan, &src, &mut out);
            }
            "KICK" => {
                let chan = p(0).to_string();
                let target = p(1).to_string();
                let reason = m.param(2).map(str::to_string);
                let l = self.line(
                    Some(chan.clone()),
                    LineKind::Kick {
                        target: target.clone(),
                        reason,
                    },
                    Some(&src),
                    "",
                    m,
                );
                out.events.push(Event::Line(l));
                self.remove_member(&chan, &target, &mut out);
            }
            "QUIT" => {
                let reason = m.param(0).map(str::to_string);
                let key = self.state.fold(&src);
                let chans: Vec<String> = self
                    .state
                    .channels
                    .values_mut()
                    .filter_map(|c| c.members.remove(&key).map(|_| c.name.clone()))
                    .collect();
                for ch in chans {
                    let l = self.line(
                        Some(ch.clone()),
                        LineKind::Quit {
                            reason: reason.clone(),
                        },
                        Some(&src),
                        "",
                        m,
                    );
                    out.events.push(Event::Line(l));
                    out.events.push(Event::Members {
                        network: self.id,
                        channel: ch,
                    });
                }
            }
            "PRIVMSG" | "NOTICE" => self.on_message(m, &src, &mut out),
            "TAGMSG" => {
                let react = m.tag("+draft/react").or_else(|| m.tag("+react"));
                let reply = m.tag("+draft/reply").or_else(|| m.tag("+reply"));
                if let (Some(emoji), Some(msgid)) = (react, reply) {
                    let buffer = self.buffer_for(p(0), &src);
                    out.events.push(Event::React {
                        network: self.id,
                        buffer,
                        msgid: msgid.to_string(),
                        nick: src.clone(),
                        emoji: emoji.to_string(),
                    });
                }
                if let Some(state) = m.tag("+typing") {
                    let buffer = self.buffer_for(p(0), &src);
                    out.events.push(Event::Typing {
                        network: self.id,
                        buffer,
                        nick: src,
                        active: state == "active",
                    });
                }
            }
            "TOPIC" => {
                let chan = p(0).to_string();
                let topic = m.param(1).filter(|t| !t.is_empty()).map(str::to_string);
                if let Some(ch) = self.state.channel_mut(&chan) {
                    ch.topic = topic.clone();
                    ch.topic_setter = Some(src.clone());
                }
                let l = self.line(
                    Some(chan.clone()),
                    LineKind::Topic,
                    Some(&src),
                    topic.as_deref().unwrap_or(""),
                    m,
                );
                out.events.push(Event::Line(l));
                out.events.push(Event::Topic {
                    network: self.id,
                    channel: chan,
                    topic,
                });
            }
            "332" => {
                let chan = p(1).to_string();
                let topic = Some(p(2).to_string()).filter(|t| !t.is_empty());
                if let Some(ch) = self.state.channel_mut(&chan) {
                    ch.topic = topic.clone();
                }
                out.events.push(Event::Topic {
                    network: self.id,
                    channel: chan,
                    topic,
                });
            }
            "353" => {
                let chan = p(2).to_string();
                let entries: Vec<Member> = p(3)
                    .split(' ')
                    .filter(|s| !s.is_empty())
                    .map(|e| self.state.parse_names_entry(e))
                    .collect();
                let keys: Vec<String> = entries.iter().map(|e| self.state.fold(&e.nick)).collect();
                if let Some(ch) = self.state.channel_mut(&chan) {
                    if !ch.names_pending {
                        ch.members.clear();
                        ch.names_pending = true;
                    }
                    for (k, e) in keys.into_iter().zip(entries) {
                        ch.members.insert(k, e);
                    }
                }
            }
            "366" => {
                let chan = p(1).to_string();
                if let Some(ch) = self.state.channel_mut(&chan) {
                    ch.names_pending = false;
                }
                out.events.push(Event::Members {
                    network: self.id,
                    channel: chan,
                });
            }
            "MODE" => self.on_mode(m, &src, &mut out),
            "AWAY" => self.update_member(&src, |mem| mem.away = m.param(0).is_some(), &mut out),
            "ACCOUNT" => {
                let acct = Some(p(0).to_string()).filter(|a| a != "*");
                self.update_member(&src, |mem| mem.account = acct.clone(), &mut out)
            }
            "CHGHOST" => {
                let (u, h) = (p(0).to_string(), p(1).to_string());
                self.update_member(
                    &src,
                    |mem| {
                        mem.user = Some(u.clone());
                        mem.host = Some(h.clone());
                    },
                    &mut out,
                )
            }
            "BATCH" => {
                let r = p(0);
                if let Some(id) = r.strip_prefix('+') {
                    self.batches.insert(
                        id.to_string(),
                        (p(1).to_string(), m.params[2.min(m.params.len())..].to_vec()),
                    );
                } else if let Some(id) = r.strip_prefix('-') {
                    self.batches.remove(id);
                }
            }
            "ERROR" => {
                let l = self.line(None, LineKind::Error, None, p(0), m);
                out.events.push(Event::Line(l));
            }
            "FAIL" | "WARN" | "NOTE" => {
                let text = format!(
                    "{} {}: {}",
                    m.command,
                    p(0),
                    m.params.last().map(String::as_str).unwrap_or("")
                );
                self.server_line(&mut out, m, text);
            }
            // MONITOR presence: "nick!user@host,nick2" online, "nick,nick2" offline.
            "730" | "731" => {
                let nicks: Vec<String> = m
                    .params
                    .last()
                    .map(|s| {
                        s.split(',')
                            .map(|t| t.split('!').next().unwrap_or("").trim().to_string())
                            .filter(|n| !n.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                out.events.push(Event::Presence {
                    network: self.id,
                    nicks,
                    online: m.command == "730",
                });
            }
            // Channel LIST: gathered and published once, never printed line by line.
            "321" => self.listing.clear(),
            "322" => {
                if self.listing.len() < 50_000 {
                    self.listing.push(crate::event::ListedChannel {
                        name: p(1).to_string(),
                        users: p(2).parse().unwrap_or(0),
                        topic: m.params.get(3).cloned().unwrap_or_default(),
                    });
                }
            }
            "323" => {
                let mut channels = std::mem::take(&mut self.listing);
                channels.sort_by_key(|c| std::cmp::Reverse(c.users));
                out.events.push(Event::ChannelList {
                    network: self.id,
                    channels,
                });
            }
            cmd if cmd.len() == 3 && cmd.bytes().all(|b| b.is_ascii_digit()) => {
                self.collect_whois(cmd, m, &mut out);
                // Show remaining numerics without the leading target nick.
                let text = m
                    .params
                    .iter()
                    .skip(1)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !text.is_empty() {
                    self.server_line(&mut out, m, text);
                }
            }
            _ => {}
        }
        out
    }

    fn remove_member(&mut self, chan: &str, nick: &str, out: &mut Output) {
        if self.state.is_me(nick) {
            let key = self.state.fold(chan);
            self.state.channels.remove(&key);
            out.events.push(Event::Parted {
                network: self.id,
                channel: chan.to_string(),
            });
        } else {
            let key = self.state.fold(nick);
            if let Some(ch) = self.state.channel_mut(chan) {
                ch.members.remove(&key);
            }
            out.events.push(Event::Members {
                network: self.id,
                channel: chan.to_string(),
            });
        }
    }

    fn update_member(&mut self, nick: &str, f: impl Fn(&mut Member), out: &mut Output) {
        let key = self.state.fold(nick);
        for ch in self.state.channels.values_mut() {
            if let Some(mem) = ch.members.get_mut(&key) {
                f(mem);
                out.events.push(Event::Members {
                    network: self.id,
                    channel: ch.name.clone(),
                });
            }
        }
    }

    /// The buffer a message to `target` from `src` belongs in.
    fn buffer_for(&self, target: &str, src: &str) -> String {
        let target =
            target.trim_start_matches(|c: char| self.state.isupport.prefix_rank(c).is_some());
        if self.state.isupport.is_channel(target) || self.state.is_me(src) {
            target.to_string()
        } else {
            src.to_string()
        }
    }

    fn on_message(&mut self, m: &Message, src: &str, out: &mut Output) {
        let target = m.param(0).unwrap_or("");
        let text = m.param(1).unwrap_or("");
        let notice = m.command == "NOTICE";
        let server_notice = matches!(m.prefix, Some(Prefix::Server(_)) | None) || target == "*";

        if let Some(c) = ctcp::parse(text) {
            // Our own offers come back to us through echo-message; they are not offers to us.
            if c.command == "DCC" && self.state.is_me(src) {
                return;
            }
            if c.command == "DCC" && !notice {
                match murk_proto::dcc::parse(c.args) {
                    Some(offer) => out.events.push(Event::DccOffer {
                        network: self.id,
                        from: src.to_string(),
                        offer,
                    }),
                    None => {
                        self.server_line(out, m, format!("Unsupported DCC from {src}: {}", c.args))
                    }
                }
                return;
            }
            if c.command != "ACTION" {
                if !notice
                    && !self.state.is_me(src)
                    && let Some(reply) = ctcp_reply(&c.command, c.args)
                {
                    out.send.push(msg("NOTICE", &[src, &reply]));
                }
                self.server_line(out, m, format!("CTCP {} from {src} {}", c.command, c.args));
                return;
            }
            let buffer = self.buffer_for(target, src);
            let l = self.line(Some(buffer), LineKind::Action, Some(src), c.args, m);
            out.events.push(Event::Line(l));
            return;
        }

        if server_notice || !self.state.registered && notice {
            let l = self.line(None, LineKind::Notice, Some(src), text, m);
            out.events.push(Event::Line(l));
            return;
        }
        let buffer = self.buffer_for(target, src);
        let kind = if notice {
            LineKind::Notice
        } else {
            LineKind::Privmsg
        };
        let l = self.line(Some(buffer), kind, Some(src), text, m);
        out.events.push(Event::Line(l));
    }

    /// Gather WHOIS replies into one record, published at RPL_ENDOFWHOIS.
    fn collect_whois(&mut self, cmd: &str, m: &Message, out: &mut Output) {
        let nick = m.param(1).unwrap_or("").to_string();
        if nick.is_empty() {
            return;
        }
        let key = nick.to_lowercase();
        let last = m.params.last().cloned().unwrap_or_default();
        if cmd == "318" {
            if let Some(w) = self.whois.remove(&key) {
                out.events.push(Event::Whois {
                    network: self.id,
                    info: w,
                });
            }
            return;
        }
        let tracked = [
            "311", "312", "313", "317", "319", "330", "671", "301", "276", "338",
        ];
        if !tracked.contains(&cmd) {
            return;
        }
        // 301 also answers PRIVMSG to an away user; only record it mid-WHOIS.
        if cmd == "301" && !self.whois.contains_key(&key) {
            return;
        }
        let w = self
            .whois
            .entry(key)
            .or_insert_with(|| crate::event::Whois {
                nick: nick.clone(),
                ..Default::default()
            });
        match cmd {
            "311" => {
                w.user = m.param(2).unwrap_or("").to_string();
                w.host = m.param(3).unwrap_or("").to_string();
                w.realname = last;
            }
            "312" => {
                w.server = m.param(2).unwrap_or("").to_string();
                w.server_info = last;
            }
            "313" => w.operator = true,
            "317" => {
                w.idle_secs = m.param(2).and_then(|s| s.parse().ok());
                w.signon = m.param(3).and_then(|s| s.parse().ok());
            }
            "319" => w
                .channels
                .extend(last.split_whitespace().map(str::to_string)),
            "330" => w.account = m.param(2).map(str::to_string),
            "671" | "276" => w.secure = true,
            "301" => w.away = Some(last),
            _ => {}
        }
    }

    fn on_mode(&mut self, m: &Message, src: &str, out: &mut Output) {
        let target = m.param(0).unwrap_or("").to_string();
        let text = m.params[1.min(m.params.len())..].join(" ");
        if !self.state.isupport.is_channel(&target) {
            self.server_line(out, m, format!("Mode {target} {text}"));
            return;
        }
        let modestr = m.param(1).unwrap_or("");
        let mut args = m.params.iter().skip(2);
        let mut adding = true;
        for c in modestr.chars() {
            match c {
                '+' => adding = true,
                '-' => adding = false,
                c => {
                    let arg = if self.state.isupport.mode_takes_param(c, adding) {
                        args.next()
                    } else {
                        None
                    };
                    if let (Some(sym), Some(nick)) = (self.state.isupport.prefix_for_mode(c), arg) {
                        let nick = nick.clone();
                        self.state.set_prefix(&target, &nick, sym, adding);
                    } else if arg.is_none()
                        && let Some(ch) = self.state.channel_mut(&target)
                    {
                        ch.modes.retain(|x| x != c);
                        if adding {
                            ch.modes.push(c);
                        }
                    }
                }
            }
        }
        let l = self.line(Some(target.clone()), LineKind::Mode, Some(src), &text, m);
        out.events.push(Event::Line(l));
        out.events.push(Event::Members {
            network: self.id,
            channel: target,
        });
    }

    fn on_cap(&mut self, m: &Message, out: &mut Output) {
        let sub = m.param(1).unwrap_or("").to_ascii_uppercase();
        let last = m.params.last().map(String::as_str).unwrap_or("");
        let more = m.param(2) == Some("*");
        match sub.as_str() {
            "LS" | "NEW" => {
                for tok in last.split(' ').filter(|s| !s.is_empty()) {
                    let (k, v) = match tok.split_once('=') {
                        Some((k, v)) => (k.to_string(), Some(v.to_string())),
                        None => (tok.to_string(), None),
                    };
                    self.offered.insert(k, v);
                }
                if more {
                    return;
                }
                let want: Vec<&str> = WANTED_CAPS
                    .iter()
                    .copied()
                    .filter(|c| self.offered.contains_key(*c) && !self.state.has_cap(c))
                    .filter(|c| *c != "sasl" || self.sasl_usable())
                    .collect();
                if want.is_empty() {
                    if sub == "LS" {
                        self.end_cap(out);
                    }
                    return;
                }
                // Keep each REQ line well under the length limit.
                let mut batches: Vec<String> = Vec::new();
                for c in want {
                    match batches.last_mut() {
                        Some(b) if b.len() + c.len() < 400 => {
                            b.push(' ');
                            b.push_str(c);
                        }
                        _ => batches.push(c.to_string()),
                    }
                }
                if sub == "LS" {
                    self.cap_phase = CapPhase::Requesting(batches.len());
                }
                for b in batches {
                    out.send.push(msg("CAP", &["REQ", &b]));
                }
            }
            "ACK" => {
                for c in last.split(' ').filter(|s| !s.is_empty()) {
                    if let Some(off) = c.strip_prefix('-') {
                        self.state.caps.remove(off);
                    } else {
                        self.state.caps.insert(c.to_string());
                    }
                }
                self.req_answered(out);
            }
            "NAK" => self.req_answered(out),
            "DEL" => {
                for c in last.split(' ') {
                    self.state.caps.remove(c);
                    self.offered.remove(c);
                }
            }
            _ => {}
        }
    }

    fn sasl_usable(&self) -> bool {
        let Some(sasl) = &self.config.sasl else {
            return false;
        };
        let mech = match sasl {
            Sasl::Plain { .. } => "PLAIN",
            Sasl::External => "EXTERNAL",
        };
        // Pre-3.2 servers send `sasl` without a mechanism list.
        match self.offered.get("sasl") {
            Some(Some(list)) => list.split(',').any(|m| m.eq_ignore_ascii_case(mech)),
            _ => true,
        }
    }

    fn req_answered(&mut self, out: &mut Output) {
        if let CapPhase::Requesting(n) = self.cap_phase {
            if n > 1 {
                self.cap_phase = CapPhase::Requesting(n - 1);
                return;
            }
            if self.state.has_cap("sasl") && self.config.sasl.is_some() {
                self.cap_phase = CapPhase::Authenticating;
                let mech = match self.config.sasl {
                    Some(Sasl::External) => "EXTERNAL",
                    _ => "PLAIN",
                };
                out.send.push(msg("AUTHENTICATE", &[mech]));
            } else {
                self.end_cap(out);
            }
        }
    }

    fn on_authenticate(&mut self, m: &Message, out: &mut Output) {
        if m.param(0) != Some("+") || self.cap_phase != CapPhase::Authenticating {
            return;
        }
        let payload = match &self.config.sasl {
            Some(Sasl::Plain { username, password }) => {
                format!("{username}\0{username}\0{password}")
            }
            _ => String::new(),
        };
        for chunk in sasl_chunks(payload.as_bytes()) {
            out.send.push(msg("AUTHENTICATE", &[&chunk]));
        }
    }

    fn end_cap(&mut self, out: &mut Output) {
        if self.cap_phase != CapPhase::Done {
            self.cap_phase = CapPhase::Done;
            out.send.push(msg("CAP", &["END"]));
        }
    }

    /// Translate a frontend command into protocol lines and local events.
    pub fn command(&mut self, cmd: Command) -> Output {
        let mut out = Output::default();
        match cmd {
            Command::Say {
                target,
                text,
                action,
            } => {
                for chunk in split_message(&text, self.max_text_len(&target)) {
                    let body = if action {
                        ctcp::encode("ACTION", &chunk)
                    } else {
                        chunk.clone()
                    };
                    out.send.push(msg("PRIVMSG", &[&target, &body]));
                    if !self.state.has_cap("echo-message") {
                        let kind = if action {
                            LineKind::Action
                        } else {
                            LineKind::Privmsg
                        };
                        out.events
                            .push(Event::Line(self.local_line(&target, kind, &chunk)));
                    }
                }
            }
            Command::Notice { target, text } => {
                for chunk in split_message(&text, self.max_text_len(&target)) {
                    out.send.push(msg("NOTICE", &[&target, &chunk]));
                    if !self.state.has_cap("echo-message") {
                        out.events.push(Event::Line(self.local_line(
                            &target,
                            LineKind::Notice,
                            &chunk,
                        )));
                    }
                }
            }
            Command::Join { channel, key } => match key {
                Some(k) => out.send.push(msg("JOIN", &[&channel, &k])),
                None => out.send.push(msg("JOIN", &[&channel])),
            },
            Command::Part { channel, reason } => match reason {
                Some(r) => out.send.push(msg("PART", &[&channel, &r])),
                None => out.send.push(msg("PART", &[&channel])),
            },
            Command::Nick(n) => out.send.push(msg("NICK", &[&n])),
            Command::Raw(line) => {
                if let Ok(m) = Message::parse(&line) {
                    out.send.push(m);
                }
            }
            Command::History {
                target,
                before,
                limit,
            } => {
                if self.state.has_cap("chathistory") || self.state.has_cap("draft/chathistory") {
                    let anchor = match before {
                        Some(b) if b.contains('T') => format!("timestamp={b}"),
                        Some(b) => format!("msgid={b}"),
                        None => "*".to_string(),
                    };
                    let verb = if anchor == "*" { "LATEST" } else { "BEFORE" };
                    let limit = limit.to_string();
                    let args: Vec<&str> = if verb == "LATEST" {
                        vec![verb, &target, "*", &limit]
                    } else {
                        vec![verb, &target, &anchor, &limit]
                    };
                    out.send.push(msg("CHATHISTORY", &args));
                }
            }
            Command::Typing { target, active } => {
                if self.state.has_cap("message-tags") {
                    let v = if active { "active" } else { "done" };
                    out.send
                        .push(msg("TAGMSG", &[&target]).with_tag("+typing", Some(v.into())));
                }
            }
            Command::Disconnect { reason } => {
                let r = reason.unwrap_or_else(|| "Murk".into());
                out.send.push(msg("QUIT", &[&r]));
            }
            Command::Connect => {}
            Command::React {
                target,
                msgid,
                emoji,
            } => {
                out.send.push(
                    msg("TAGMSG", &[&target])
                        .with_tag("+draft/react", Some(emoji.clone()))
                        .with_tag("+draft/reply", Some(msgid.clone())),
                );
                // Shown at once: not every server echoes tag-only messages, and
                // a later echo is a no-op because reactions dedupe per nick.
                {
                    out.events.push(Event::React {
                        network: self.id,
                        buffer: target,
                        msgid,
                        nick: self.state.nick.clone(),
                        emoji,
                    });
                }
            }
            Command::Reply {
                target,
                msgid,
                text,
            } => {
                for chunk in split_message(&text, self.max_text_len(&target).saturating_sub(64)) {
                    out.send.push(
                        msg("PRIVMSG", &[&target, &chunk])
                            .with_tag("+draft/reply", Some(msgid.clone())),
                    );
                    if !self.state.has_cap("echo-message") {
                        let mut l = self.local_line(&target, LineKind::Privmsg, &chunk);
                        l.reply_to = Some(msgid.clone());
                        out.events.push(Event::Line(l));
                    }
                }
            }
        }
        out
    }

    fn local_line(&self, target: &str, kind: LineKind, text: &str) -> Line {
        Line {
            network: self.id,
            buffer: Some(target.to_string()),
            kind,
            sender: Some(self.state.nick.clone()),
            text: text.to_string(),
            time: SystemTime::now(),
            msgid: None,
            own: true,
            highlight: false,
            history: false,
            reply_to: None,
        }
    }

    /// Room left for text once the server prepends `:nick!user@host PRIVMSG target :`.
    fn max_text_len(&self, target: &str) -> usize {
        // Hostmask length is unknown until WHO; assume a generous 63-byte host and 10-byte user.
        let overhead =
            1 + self.state.nick.len() + 1 + 10 + 1 + 63 + " PRIVMSG ".len() + target.len() + 2;
        murk_proto::message::MAX_BODY_LEN
            .saturating_sub(overhead)
            .max(64)
    }
}

fn msg(cmd: &str, params: &[&str]) -> Message {
    Message::new(cmd, params.iter().map(|s| s.to_string()).collect())
}

fn user_host(m: &Message) -> (Option<String>, Option<String>) {
    match &m.prefix {
        Some(Prefix::User { user, host, .. }) => (user.clone(), host.clone()),
        _ => (None, None),
    }
}

fn contains_word(hay: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || "_-[]\\`^{}|".contains(c);
    hay.match_indices(word).any(|(i, _)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

fn ctcp_reply(command: &str, args: &str) -> Option<String> {
    let body = match command {
        "VERSION" => format!("Murk {}", env!("CARGO_PKG_VERSION")),
        "PING" => args.to_string(),
        "TIME" => crate::time::format(SystemTime::now()),
        "CLIENTINFO" => "ACTION CLIENTINFO PING TIME VERSION".to_string(),
        _ => return None,
    };
    Some(ctcp::encode(command, &body))
}

/// Base64-encode and split into AUTHENTICATE chunks; a payload that is an exact
/// multiple of the chunk size is terminated by a lone `+`.
pub fn sasl_chunks(payload: &[u8]) -> Vec<String> {
    if payload.is_empty() {
        return vec!["+".into()];
    }
    let enc = base64::engine::general_purpose::STANDARD.encode(payload);
    let mut chunks: Vec<String> = enc
        .as_bytes()
        .chunks(SASL_CHUNK)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    if enc.len().is_multiple_of(SASL_CHUNK) {
        chunks.push("+".into());
    }
    chunks
}

/// Split text into pieces of at most `max` bytes on char boundaries, preferring spaces.
pub fn split_message(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split(['\n', '\r']).filter(|l| !l.is_empty()) {
        let mut rest = line;
        while rest.len() > max {
            let mut cut = max;
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            if let Some(sp) = rest[..cut].rfind(' ').filter(|&i| i > max / 2) {
                cut = sp;
            }
            out.push(rest[..cut].to_string());
            rest = rest[cut..].trim_start_matches(' ');
        }
        if !rest.is_empty() {
            out.push(rest.to_string());
        }
    }
    out
}
