//! The API between the engine and frontends.

use murk_proto::Message;
use std::time::SystemTime;

pub type NetworkId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Privmsg,
    Notice,
    Action,
    Join,
    Part {
        reason: Option<String>,
    },
    Quit {
        reason: Option<String>,
    },
    Kick {
        target: String,
        reason: Option<String>,
    },
    Nick {
        new: String,
    },
    Topic,
    Mode,
    /// Server numerics, MOTD and anything else shown in the server buffer.
    Server,
    Error,
}

/// One displayable line in a buffer.
#[derive(Debug, Clone)]
pub struct Line {
    pub network: NetworkId,
    /// Channel name, query nick, or `None` for the server buffer.
    pub buffer: Option<String>,
    pub kind: LineKind,
    pub sender: Option<String>,
    pub text: String,
    pub time: SystemTime,
    pub msgid: Option<String>,
    /// True when this line came from our own client (echo or local).
    pub own: bool,
    /// True when the text mentions our nick or a highlight word.
    pub highlight: bool,
    /// Set for lines replayed from chathistory/bouncer playback.
    pub history: bool,
    /// msgid of the message this one answers (`+draft/reply`).
    pub reply_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Registering,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone)]
pub enum Event {
    State {
        network: NetworkId,
        state: ConnState,
        detail: Option<String>,
    },
    Line(Line),
    /// Our nick changed (including the initial registration result).
    OwnNick {
        network: NetworkId,
        nick: String,
    },
    Joined {
        network: NetworkId,
        channel: String,
    },
    Parted {
        network: NetworkId,
        channel: String,
    },
    Topic {
        network: NetworkId,
        channel: String,
        topic: Option<String>,
    },
    /// The member list of a channel changed; frontends re-read it from state.
    Members {
        network: NetworkId,
        channel: String,
    },
    Typing {
        network: NetworkId,
        buffer: String,
        nick: String,
        active: bool,
    },
    /// An emoji reaction (`+draft/react`) to an earlier message.
    React {
        network: NetworkId,
        buffer: String,
        msgid: String,
        nick: String,
        emoji: String,
    },
    /// Every message as received, for raw view and scripts.
    Raw {
        network: NetworkId,
        msg: Message,
        outgoing: bool,
    },
}

#[derive(Debug, Clone)]
pub enum Command {
    Connect,
    Disconnect {
        reason: Option<String>,
    },
    /// Send text to a target, as typed into a buffer. Handles `/me` via `action`.
    Say {
        target: String,
        text: String,
        action: bool,
    },
    Notice {
        target: String,
        text: String,
    },
    Join {
        channel: String,
        key: Option<String>,
    },
    Part {
        channel: String,
        reason: Option<String>,
    },
    Nick(String),
    /// A raw protocol line, as sent by `/quote`.
    Raw(String),
    /// Ask the server for older history before a msgid or timestamp.
    History {
        target: String,
        before: Option<String>,
        limit: u32,
    },
    Typing {
        target: String,
        active: bool,
    },
    /// React to a message by msgid.
    React {
        target: String,
        msgid: String,
        emoji: String,
    },
    /// Send text as a reply to a message by msgid.
    Reply {
        target: String,
        msgid: String,
        text: String,
    },
}
