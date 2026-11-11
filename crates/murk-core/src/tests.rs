use crate::config::{NetworkConfig, Sasl};
use crate::event::{Command, ConnState, Event, LineKind};
use crate::session::{Session, sasl_chunks, split_message};
use murk_proto::Message;

fn session() -> Session {
    let mut c = NetworkConfig::new("test", "localhost", "murk");
    c.autojoin = vec!["#a".into()];
    Session::new(1, c)
}

fn feed(s: &mut Session, line: &str) -> crate::session::Output {
    s.handle(&Message::parse(line).unwrap())
}

fn wires(out: &crate::session::Output) -> Vec<String> {
    out.send.iter().map(Message::to_wire).collect()
}

fn lines(out: &crate::session::Output) -> Vec<crate::event::Line> {
    out.events
        .iter()
        .filter_map(|e| match e {
            Event::Line(l) => Some(l.clone()),
            _ => None,
        })
        .collect()
}

fn register(s: &mut Session) {
    s.start();
    feed(
        s,
        ":srv CAP * LS :multi-prefix server-time echo-message message-tags",
    );
    feed(s, ":srv CAP * ACK :multi-prefix server-time message-tags");
    feed(
        s,
        ":srv 005 murk PREFIX=(qaohv)~&@%+ CHANTYPES=# :are supported",
    );
    feed(s, ":srv 001 murk :Welcome");
}

#[test]
fn start_sends_registration() {
    let mut s = session();
    let out = s.start();
    assert_eq!(
        wires(&out),
        vec!["CAP LS 302", "NICK murk", "USER murk 0 * murk"]
    );
}

#[test]
fn cap_negotiation_multiline_and_end() {
    let mut s = session();
    s.start();
    let out = feed(&mut s, ":srv CAP * LS * :multi-prefix sasl=PLAIN");
    assert!(out.send.is_empty());
    let out = feed(&mut s, ":srv CAP * LS :server-time unknown-cap");
    // No SASL configured, so sasl is not requested.
    assert_eq!(wires(&out), vec!["CAP REQ :multi-prefix server-time"]);
    let out = feed(&mut s, ":srv CAP * ACK :multi-prefix server-time");
    assert_eq!(wires(&out), vec!["CAP END"]);
    assert!(s.state.has_cap("server-time"));
}

#[test]
fn no_caps_offered_ends_immediately() {
    let mut s = session();
    s.start();
    assert_eq!(wires(&feed(&mut s, ":srv CAP * LS :foo")), vec!["CAP END"]);
}

#[test]
fn sasl_plain_flow() {
    let mut c = NetworkConfig::new("t", "h", "murk");
    c.sasl = Some(Sasl::Plain {
        username: "u".into(),
        password: "p".into(),
    });
    let mut s = Session::new(1, c);
    s.start();
    assert_eq!(
        wires(&feed(&mut s, ":srv CAP * LS :sasl=PLAIN,EXTERNAL")),
        vec!["CAP REQ sasl"]
    );
    assert_eq!(
        wires(&feed(&mut s, ":srv CAP * ACK sasl")),
        vec!["AUTHENTICATE PLAIN"]
    );
    // base64("u\0u\0p")
    assert_eq!(
        wires(&feed(&mut s, "AUTHENTICATE +")),
        vec!["AUTHENTICATE dQB1AHA="]
    );
    assert_eq!(
        wires(&feed(&mut s, ":srv 903 murk :SASL successful")),
        vec!["CAP END"]
    );
}

#[test]
fn sasl_skipped_when_mech_unsupported() {
    let mut c = NetworkConfig::new("t", "h", "murk");
    c.sasl = Some(Sasl::External);
    let mut s = Session::new(1, c);
    s.start();
    assert_eq!(
        wires(&feed(&mut s, ":srv CAP * LS :sasl=PLAIN")),
        vec!["CAP END"]
    );
}

#[test]
fn sasl_chunking() {
    assert_eq!(sasl_chunks(b""), vec!["+"]);
    // 300 bytes encode to exactly 400 base64 chars, so a terminating "+" follows.
    let c = sasl_chunks(&[0u8; 300]);
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].len(), 400);
    assert_eq!(c[1], "+");
}

#[test]
fn nick_in_use_falls_back() {
    let mut s = session();
    s.config.alt_nicks = vec!["murk2".into()];
    s.start();
    assert_eq!(
        wires(&feed(&mut s, ":srv 433 * murk :in use")),
        vec!["NICK murk2"]
    );
    assert_eq!(
        wires(&feed(&mut s, ":srv 433 * murk2 :in use")),
        vec!["NICK murk__"]
    );
}

#[test]
fn welcome_autojoins_and_sets_nick() {
    let mut s = session();
    s.start();
    let out = feed(&mut s, ":srv 001 murk_ :Welcome");
    assert_eq!(s.state.nick, "murk_");
    assert!(wires(&out).contains(&"JOIN #a".to_string()));
    assert!(out.events.iter().any(|e| matches!(
        e,
        Event::State {
            state: ConnState::Connected,
            ..
        }
    )));
}

#[test]
fn ping_pong() {
    let mut s = session();
    assert_eq!(wires(&feed(&mut s, "PING :abc")), vec!["PONG abc"]);
}

#[test]
fn join_names_mode_part() {
    let mut s = session();
    register(&mut s);
    feed(&mut s, ":murk!u@h JOIN #a");
    assert!(s.state.channel("#A").is_some());
    feed(&mut s, ":srv 353 murk = #a :~@alice +bob murk");
    feed(&mut s, ":srv 366 murk #a :End");
    let ch = s.state.channel("#a").unwrap();
    assert_eq!(ch.members.len(), 3);
    assert_eq!(ch.members["alice"].prefixes, "~@");

    feed(&mut s, ":alice!a@h MODE #a +o-v bob bob");
    assert_eq!(s.state.channel("#a").unwrap().members["bob"].prefixes, "@");
    feed(&mut s, ":alice!a@h MODE #a +h bob");
    assert_eq!(s.state.channel("#a").unwrap().members["bob"].prefixes, "@%");
    feed(&mut s, ":alice!a@h MODE #a +ntk key");
    assert_eq!(s.state.channel("#a").unwrap().modes, "nt");

    let order: Vec<String> = s
        .state
        .sorted_members("#a")
        .into_iter()
        .map(|m| m.nick)
        .collect();
    assert_eq!(order, vec!["alice", "bob", "murk"]);

    feed(&mut s, ":bob!b@h NICK robert");
    assert!(
        s.state
            .channel("#a")
            .unwrap()
            .members
            .contains_key("robert")
    );
    feed(&mut s, ":robert!b@h QUIT :bye");
    assert_eq!(s.state.channel("#a").unwrap().members.len(), 2);

    let out = feed(&mut s, ":murk!u@h PART #a :done");
    assert!(s.state.channel("#a").is_none());
    assert!(out.events.iter().any(|e| matches!(e, Event::Parted { .. })));
}

#[test]
fn messages_route_to_buffers() {
    let mut s = session();
    register(&mut s);
    let out = feed(
        &mut s,
        "@time=2026-01-01T00:00:00.000Z;msgid=x :bob!b@h PRIVMSG #a :hey murk!",
    );
    let l = &lines(&out)[0];
    assert_eq!(l.buffer.as_deref(), Some("#a"));
    assert!(l.highlight);
    assert_eq!(l.msgid.as_deref(), Some("x"));
    assert_eq!(crate::time::format(l.time), "2026-01-01T00:00:00.000Z");

    let out = feed(&mut s, ":bob!b@h PRIVMSG murk :private");
    assert_eq!(lines(&out)[0].buffer.as_deref(), Some("bob"));

    let out = feed(&mut s, ":bob!b@h PRIVMSG #a :murky waters");
    assert!(!lines(&out)[0].highlight);

    let out = feed(&mut s, ":bob!b@h PRIVMSG #a :\x01ACTION waves\x01");
    assert_eq!(lines(&out)[0].kind, LineKind::Action);

    let out = feed(&mut s, ":srv NOTICE * :Looking up your hostname");
    assert_eq!(lines(&out)[0].buffer, None);

    let out = feed(&mut s, ":bob!b@h PRIVMSG murk :\x01VERSION\x01");
    assert!(wires(&out)[0].starts_with("NOTICE bob :\x01VERSION Murk"));
}

#[test]
fn say_echoes_locally_without_echo_message() {
    let mut s = session();
    register(&mut s);
    let out = s.command(Command::Say {
        target: "#a".into(),
        text: "hi\nthere".into(),
        action: false,
    });
    assert_eq!(wires(&out), vec!["PRIVMSG #a hi", "PRIVMSG #a there"]);
    assert_eq!(lines(&out).len(), 2);
    assert!(lines(&out)[0].own);

    s.state.caps.insert("echo-message".into());
    let out = s.command(Command::Say {
        target: "#a".into(),
        text: "x".into(),
        action: true,
    });
    assert_eq!(wires(&out), vec!["PRIVMSG #a :\x01ACTION x\x01"]);
    assert!(lines(&out).is_empty());
}

#[test]
fn split_respects_utf8_and_spaces() {
    let parts = split_message("aaaa bbbb cccc", 10);
    assert_eq!(parts, vec!["aaaa bbbb", "cccc"]);
    let parts = split_message(&"é".repeat(10), 5);
    assert!(parts.iter().all(|p| p.len() <= 5 && !p.is_empty()));
    assert_eq!(parts.concat(), "é".repeat(10));
}

#[test]
fn history_batch_marks_lines() {
    let mut s = session();
    register(&mut s);
    feed(&mut s, ":srv BATCH +b1 chathistory #a");
    let out = feed(&mut s, "@batch=b1 :bob!b@h PRIVMSG #a :old");
    assert!(lines(&out)[0].history);
    feed(&mut s, ":srv BATCH -b1");
    let out = feed(&mut s, ":bob!b@h PRIVMSG #a :new");
    assert!(!lines(&out)[0].history);
}

#[tokio::test]
async fn engine_talks_to_a_server() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (r, mut w) = sock.into_split();
        let mut r = BufReader::new(r).lines();
        let mut got = Vec::new();
        while let Some(l) = r.next_line().await.unwrap() {
            got.push(l.clone());
            if l.starts_with("USER") {
                w.write_all(b":srv CAP * LS :\r\n:srv 001 murk :Welcome\r\n")
                    .await
                    .unwrap();
            }
            if l.starts_with("JOIN") {
                w.write_all(b":murk!u@h JOIN #a\r\n:bob!b@h PRIVMSG #a :hello\r\n")
                    .await
                    .unwrap();
            }
            if l.starts_with("QUIT") {
                break;
            }
        }
        got
    });

    let engine = crate::Engine::new();
    let mut events = engine.subscribe();
    let mut cfg = NetworkConfig::new("local", "127.0.0.1", "murk");
    cfg.port = port;
    cfg.tls = false;
    cfg.reconnect = false;
    cfg.autojoin = vec!["#a".into()];
    let id = engine.add_network(cfg);

    let hello = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Event::Line(l) = events.recv().await.unwrap()
                && l.text == "hello"
            {
                return l;
            }
        }
    })
    .await
    .expect("message from fake server");
    assert_eq!(hello.buffer.as_deref(), Some("#a"));
    assert!(
        engine
            .with_state(id, |s| s.channel("#a").is_some())
            .unwrap()
    );

    engine.send(
        id,
        Command::Disconnect {
            reason: Some("bye".into()),
        },
    );
    let got = server.await.unwrap();
    assert!(got.contains(&"CAP END".to_string()));
    assert!(got.contains(&"QUIT bye".to_string()));
}
