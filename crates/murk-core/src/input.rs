//! Parsing of typed input lines into actions, shared by every frontend.

use crate::event::Command;

/// What the user asked for. `Send` goes to the engine for the current network;
/// the rest are handled by the frontend.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Send(CommandEq),
    /// Open (and focus) a query buffer, optionally sending a first message.
    Query {
        nick: String,
        text: Option<String>,
    },
    Close,
    Connect(Option<String>),
    Disconnect(Option<String>),
    Search(String),
    Clear,
    Help(Option<String>),
    Quit(Option<String>),
    /// Switch to buffer by number or name.
    Window(String),
    Error(String),
    Nothing,
}

/// `Command` wrapper with equality for testing.
#[derive(Debug, Clone)]
pub struct CommandEq(pub Command);

impl PartialEq for CommandEq {
    fn eq(&self, other: &Self) -> bool {
        format!("{:?}", self.0) == format!("{:?}", other.0)
    }
}

pub const HELP: &[(&str, &str)] = &[
    ("join", "/join <#channel>[,#more] [key]"),
    ("list", "/list [*word*|>users]: find channels"),
    ("part", "/part [#channel] [reason]"),
    ("msg", "/msg <target> <text>"),
    ("query", "/query <nick> [text]"),
    ("me", "/me <action>"),
    ("react", "/react <emoji>: react to the last message"),
    ("reply", "/reply <text>: reply to the last message"),
    (
        "dcc",
        "/dcc [list | get [id] | decline <id> | send <nick> <path>]",
    ),
    (
        "theme",
        "/theme [name]: murk, aurora, ember, synthwave, mocha, phosphor, paper",
    ),
    ("notice", "/notice <target> <text>"),
    ("nick", "/nick <newnick>"),
    ("topic", "/topic [new topic]"),
    ("mode", "/mode <target> <modes> [args]"),
    ("kick", "/kick <nick> [reason]"),
    ("op", "/op <nick>..., also /deop /voice /devoice"),
    ("ban", "/ban <mask>, /unban <mask>"),
    ("invite", "/invite <nick> [#channel]"),
    ("whois", "/whois <nick>"),
    ("away", "/away [message] (no message clears)"),
    ("quote", "/quote <raw line>"),
    ("close", "/close: leave and close the current buffer"),
    ("connect", "/connect [network]"),
    ("disconnect", "/disconnect [reason]"),
    ("search", "/search <words>"),
    (
        "history",
        "/history [count]: fetch older lines from the server",
    ),
    ("window", "/window <n|name>, also Alt+1..9"),
    ("clear", "/clear"),
    ("quit", "/quit [message]"),
];

fn send(c: Command) -> Action {
    Action::Send(CommandEq(c))
}

fn raw(line: String) -> Action {
    send(Command::Raw(line))
}

/// `target` is the current buffer (channel or nick), `None` in a server buffer.
/// `is_channel` decides whether a word names a channel on this network.
pub fn parse(text: &str, target: Option<&str>, is_channel: impl Fn(&str) -> bool) -> Action {
    if text.is_empty() {
        return Action::Nothing;
    }
    // "//text" sends a literal line starting with "/".
    let Some(body) = text.strip_prefix('/').filter(|b| !b.starts_with('/')) else {
        let text = text.strip_prefix('/').unwrap_or(text);
        return match target {
            Some(t) => send(Command::Say {
                target: t.into(),
                text: text.into(),
                action: false,
            }),
            None => Action::Error("Not in a channel or query; use /msg or /join".into()),
        };
    };

    let (cmd, rest) = body.split_once(' ').unwrap_or((body, ""));
    let rest = rest.trim();
    let (arg1, arg2) = rest
        .split_once(' ')
        .map(|(a, b)| (a, b.trim()))
        .unwrap_or((rest, ""));
    let need_target = || {
        target
            .map(str::to_string)
            .ok_or("No current channel or query")
    };
    let chan_or_current = |word: &str| -> Option<(String, bool)> {
        if is_channel(word) {
            Some((word.to_string(), true))
        } else {
            target.map(|t| (t.to_string(), false))
        }
    };
    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());

    match cmd.to_ascii_lowercase().as_str() {
        "join" | "j" => {
            if arg1.is_empty() {
                return Action::Error("Usage: /join <#channel> [key]".into());
            }
            let channel = if is_channel(arg1) || arg1.contains(',') {
                arg1.to_string()
            } else {
                format!("#{arg1}")
            };
            send(Command::Join {
                channel,
                key: opt(arg2),
            })
        }
        "part" | "leave" => match chan_or_current(arg1) {
            Some((ch, explicit)) => {
                let reason = if explicit { arg2 } else { rest };
                send(Command::Part {
                    channel: ch,
                    reason: opt(reason),
                })
            }
            None => Action::Error("Usage: /part [#channel] [reason]".into()),
        },
        "msg" | "privmsg" if !arg1.is_empty() && !arg2.is_empty() => send(Command::Say {
            target: arg1.into(),
            text: arg2.into(),
            action: false,
        }),
        "query" | "q" if !arg1.is_empty() => Action::Query {
            nick: arg1.into(),
            text: opt(arg2),
        },
        "me" | "action" => match need_target() {
            Ok(t) => send(Command::Say {
                target: t,
                text: rest.into(),
                action: true,
            }),
            Err(e) => Action::Error(e.into()),
        },
        "notice" if !arg1.is_empty() && !arg2.is_empty() => send(Command::Notice {
            target: arg1.into(),
            text: arg2.into(),
        }),
        "nick" if !arg1.is_empty() => send(Command::Nick(arg1.into())),
        "topic" | "t" => match chan_or_current(arg1) {
            Some((ch, explicit)) => {
                let topic = if explicit { arg2 } else { rest };
                if topic.is_empty() {
                    raw(format!("TOPIC {ch}"))
                } else {
                    raw(format!("TOPIC {ch} :{topic}"))
                }
            }
            None => Action::Error("Usage: /topic [#channel] [topic]".into()),
        },
        "mode" if !rest.is_empty() => {
            if is_channel(arg1) || !arg1.starts_with(['+', '-']) {
                raw(format!("MODE {rest}"))
            } else {
                match need_target() {
                    Ok(t) => raw(format!("MODE {t} {rest}")),
                    Err(e) => Action::Error(e.into()),
                }
            }
        }
        m @ ("op" | "deop" | "voice" | "devoice" | "halfop" | "dehalfop") if !rest.is_empty() => {
            let Ok(t) = need_target() else {
                return Action::Error("Not in a channel".into());
            };
            let letter = match m.trim_start_matches("de") {
                "op" => 'o',
                "voice" => 'v',
                _ => 'h',
            };
            let sign = if m.starts_with("de") { '-' } else { '+' };
            let nicks: Vec<&str> = rest.split_whitespace().collect();
            let modes: String = std::iter::once(sign)
                .chain(nicks.iter().map(|_| letter))
                .collect();
            raw(format!("MODE {t} {modes} {}", nicks.join(" ")))
        }
        b @ ("ban" | "unban") if !arg1.is_empty() => match need_target() {
            Ok(t) => {
                let mask = if arg1.contains(['!', '@']) {
                    arg1.to_string()
                } else {
                    format!("{arg1}!*@*")
                };
                let sign = if b == "ban" { '+' } else { '-' };
                raw(format!("MODE {t} {sign}b {mask}"))
            }
            Err(e) => Action::Error(e.into()),
        },
        "kick" | "k" if !arg1.is_empty() => match need_target() {
            Ok(t) if arg2.is_empty() => raw(format!("KICK {t} {arg1}")),
            Ok(t) => raw(format!("KICK {t} {arg1} :{arg2}")),
            Err(e) => Action::Error(e.into()),
        },
        "invite" if !arg1.is_empty() => {
            let ch = if arg2.is_empty() {
                target.unwrap_or("").to_string()
            } else {
                arg2.to_string()
            };
            raw(format!("INVITE {arg1} {ch}"))
        }
        "whois" | "wi" if !arg1.is_empty() => raw(format!("WHOIS {arg1}")),
        "who" if !arg1.is_empty() => raw(format!("WHO {arg1}")),
        "away" => {
            if rest.is_empty() {
                raw("AWAY".into())
            } else {
                raw(format!("AWAY :{rest}"))
            }
        }
        "quote" | "raw" if !rest.is_empty() => raw(rest.into()),
        "ctcp" if !arg1.is_empty() && !arg2.is_empty() => {
            let (c, a) = arg2.split_once(' ').unwrap_or((arg2, ""));
            let body = murk_proto::ctcp::encode(&c.to_ascii_uppercase(), a);
            send(Command::Say {
                target: arg1.into(),
                text: body,
                action: false,
            })
        }
        "history" => match need_target() {
            Ok(t) => send(Command::History {
                target: t,
                before: None,
                limit: arg1.parse().unwrap_or(100),
            }),
            Err(e) => Action::Error(e.into()),
        },
        "close" | "wc" => Action::Close,
        "connect" | "server" => Action::Connect(opt(rest)),
        "disconnect" => Action::Disconnect(opt(rest)),
        "search" | "grep" if !rest.is_empty() => Action::Search(rest.into()),
        "clear" => Action::Clear,
        "window" | "win" | "w" | "buffer" | "b" if !arg1.is_empty() => Action::Window(arg1.into()),
        "help" | "h" => Action::Help(opt(arg1)),
        "quit" | "exit" => Action::Quit(opt(rest)),
        "list" => raw(format!("LIST {rest}").trim_end().to_string()),
        other => {
            if HELP.iter().any(|(n, _)| *n == other) {
                let usage = HELP
                    .iter()
                    .find(|(n, _)| *n == other)
                    .map(|(_, u)| *u)
                    .unwrap_or("");
                Action::Error(format!("Usage: {usage}"))
            } else {
                // Unknown commands go to the server verbatim, as most clients do.
                raw(format!("{} {rest}", other.to_ascii_uppercase())
                    .trim_end()
                    .to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Action {
        parse(text, Some("#murk"), |s| s.starts_with('#'))
    }

    fn raw_of(a: Action) -> String {
        match a {
            Action::Send(CommandEq(Command::Raw(r))) => r,
            other => panic!("not raw: {other:?}"),
        }
    }

    #[test]
    fn list_goes_to_the_server() {
        assert_eq!(raw_of(p("/list *rust*")), "LIST *rust*");
        assert_eq!(raw_of(p("/list")), "LIST");
    }

    #[test]
    fn plain_text_and_escape() {
        assert_eq!(
            p("hi"),
            send(Command::Say {
                target: "#murk".into(),
                text: "hi".into(),
                action: false
            })
        );
        assert_eq!(
            p("//etc"),
            send(Command::Say {
                target: "#murk".into(),
                text: "/etc".into(),
                action: false
            })
        );
        assert!(matches!(parse("hi", None, |_| false), Action::Error(_)));
    }

    #[test]
    fn channel_commands() {
        assert_eq!(
            p("/join rust"),
            send(Command::Join {
                channel: "#rust".into(),
                key: None
            })
        );
        assert_eq!(
            p("/part bye all"),
            send(Command::Part {
                channel: "#murk".into(),
                reason: Some("bye all".into())
            })
        );
        assert_eq!(
            p("/part #x"),
            send(Command::Part {
                channel: "#x".into(),
                reason: None
            })
        );
        assert_eq!(raw_of(p("/topic new topic")), "TOPIC #murk :new topic");
        assert_eq!(raw_of(p("/op a b")), "MODE #murk +oo a b");
        assert_eq!(raw_of(p("/devoice a")), "MODE #murk -v a");
        assert_eq!(raw_of(p("/ban eve")), "MODE #murk +b eve!*@*");
        assert_eq!(raw_of(p("/kick eve go away")), "KICK #murk eve :go away");
        assert_eq!(raw_of(p("/mode +m")), "MODE #murk +m");
    }

    #[test]
    fn client_actions() {
        assert_eq!(
            p("/query bob hey"),
            Action::Query {
                nick: "bob".into(),
                text: Some("hey".into())
            }
        );
        assert_eq!(p("/quit later"), Action::Quit(Some("later".into())));
        assert_eq!(p("/w 3"), Action::Window("3".into()));
        assert!(matches!(p("/msg bob"), Action::Error(_)));
        assert_eq!(raw_of(p("/foo bar")), "FOO bar");
        assert_eq!(raw_of(p("/away")), "AWAY");
    }
}
