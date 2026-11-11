//! The async engine: one task per network, a command channel in and an event bus out.

use crate::config::NetworkConfig;
use crate::conn;
use crate::event::{Command, ConnState, Event, NetworkId};
use crate::session::{Output, Session};
use crate::state::NetworkState;
use murk_proto::Message;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

const EVENT_CAPACITY: usize = 4096;
/// Silence after which we PING the server, and then give up.
const PING_IDLE: Duration = Duration::from_secs(120);
const PING_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest we wait for queued lines (and the QUIT behind them) to be written.
pub const QUIT_DRAIN: Duration = Duration::from_secs(8);

#[derive(Clone)]
pub struct Engine {
    events: broadcast::Sender<Event>,
    networks: Arc<RwLock<HashMap<NetworkId, Handle>>>,
    next_id: Arc<AtomicU32>,
}

#[derive(Clone)]
struct Handle {
    tx: mpsc::UnboundedSender<Command>,
    state: Arc<RwLock<NetworkState>>,
    config: NetworkConfig,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Engine {
            events,
            networks: Arc::default(),
            next_id: Arc::new(AtomicU32::new(1)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Register a network and start its task. Must be called within a tokio runtime.
    pub fn add_network(&self, config: NetworkConfig) -> NetworkId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::unbounded_channel();
        let state = Arc::new(RwLock::new(NetworkState::default()));
        let handle = Handle {
            tx,
            state: state.clone(),
            config: config.clone(),
        };
        self.networks.write().unwrap().insert(id, handle);
        tokio::spawn(run_network(id, config, rx, state, self.events.clone()));
        id
    }

    pub fn send(&self, network: NetworkId, cmd: Command) -> bool {
        self.networks
            .read()
            .unwrap()
            .get(&network)
            .is_some_and(|h| h.tx.send(cmd).is_ok())
    }

    /// A snapshot of a network's tracked state.
    pub fn state(&self, network: NetworkId) -> Option<NetworkState> {
        let nets = self.networks.read().unwrap();
        nets.get(&network).map(|h| h.state.read().unwrap().clone())
    }

    pub fn with_state<R>(
        &self,
        network: NetworkId,
        f: impl FnOnce(&NetworkState) -> R,
    ) -> Option<R> {
        let nets = self.networks.read().unwrap();
        nets.get(&network).map(|h| f(&h.state.read().unwrap()))
    }

    pub fn networks(&self) -> Vec<(NetworkId, String)> {
        let mut v: Vec<_> = self
            .networks
            .read()
            .unwrap()
            .iter()
            .map(|(id, h)| (*id, h.config.name.clone()))
            .collect();
        v.sort();
        v
    }

    pub fn remove_network(&self, network: NetworkId) {
        if let Some(h) = self.networks.write().unwrap().remove(&network) {
            let _ = h.tx.send(Command::Disconnect { reason: None });
        }
    }
}

enum Exit {
    /// The user asked to disconnect, or the engine went away.
    Stop,
    /// Connection dropped; reconnect if configured.
    Lost(String),
}

async fn run_network(
    id: NetworkId,
    config: NetworkConfig,
    mut rx: mpsc::UnboundedReceiver<Command>,
    shared: Arc<RwLock<NetworkState>>,
    events: broadcast::Sender<Event>,
) {
    let mut session = Session::new(id, config);
    let mut backoff = Duration::from_secs(2);
    loop {
        let _ = events.send(Event::State {
            network: id,
            state: ConnState::Connecting,
            detail: None,
        });
        let cfg = &session.config;
        let exit = match conn::connect(&cfg.host, cfg.port, cfg.tls, cfg.tls_insecure).await {
            Ok(stream) => {
                backoff = Duration::from_secs(2);
                run_connection(&mut session, stream, &mut rx, &shared, &events).await
            }
            Err(e) => Exit::Lost(e.to_string()),
        };
        let detail = match &exit {
            Exit::Stop => None,
            Exit::Lost(e) => Some(e.clone()),
        };
        let _ = events.send(Event::State {
            network: id,
            state: ConnState::Disconnected,
            detail,
        });
        shared.write().unwrap().registered = false;

        let reconnect = matches!(exit, Exit::Lost(_)) && session.config.reconnect;
        if !reconnect {
            // Idle until the user asks to connect again, or the network is removed.
            loop {
                match rx.recv().await {
                    Some(Command::Connect) => break,
                    Some(_) => {}
                    None => return,
                }
            }
            continue;
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            cmd = rx.recv() => match cmd {
                None => return,
                Some(Command::Disconnect { .. }) => {
                    // Stay down until an explicit Connect.
                    loop {
                        match rx.recv().await {
                            Some(Command::Connect) => break,
                            Some(_) => {}
                            None => return,
                        }
                    }
                }
                Some(_) => {}
            }
        }
        backoff = (backoff * 2).min(Duration::from_secs(300));
    }
}

async fn run_connection(
    session: &mut Session,
    stream: conn::BoxStream,
    rx: &mut mpsc::UnboundedReceiver<Command>,
    shared: &Arc<RwLock<NetworkState>>,
    events: &broadcast::Sender<Event>,
) -> Exit {
    let (mut reader, writer) = conn::split(stream);
    let (queue, mut writer_task) = conn::spawn_writer(writer);
    let id = session.id;

    // Publish state and events, then queue outgoing lines.
    let flush = |out: Output, session: &Session| {
        *shared.write().unwrap() = session.state.clone();
        for e in out.events {
            let _ = events.send(e);
        }
        for m in out.send {
            let _ = events.send(Event::Raw {
                network: id,
                msg: m.clone(),
                outgoing: true,
            });
            let _ = queue.send((conn::Priority::of(&m), m.to_wire()));
        }
    };

    flush(session.start(), session);

    let mut pinged = false;
    loop {
        let timeout = if pinged { PING_TIMEOUT } else { PING_IDLE };
        tokio::select! {
            res = &mut writer_task => {
                let detail = match res {
                    Ok(Err(e)) => e.to_string(),
                    _ => "writer stopped".to_string(),
                };
                return Exit::Lost(detail);
            }
            line = tokio::time::timeout(timeout, reader.next_line()) => {
                let line = match line {
                    Err(_) if pinged => return Exit::Lost("ping timeout".into()),
                    Err(_) => {
                        pinged = true;
                        let ping = Message::new("PING", vec!["murk".into()]);
                        flush(Output { send: vec![ping], events: vec![] }, session);
                        continue;
                    }
                    Ok(Ok(Some(l))) => l,
                    Ok(Ok(None)) => return Exit::Lost("connection closed".into()),
                    Ok(Err(e)) => return Exit::Lost(e.to_string()),
                };
                pinged = false;
                let Ok(msg) = Message::parse(&line) else { continue };
                let out = session.handle(&msg);
                flush(out, session);
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { return Exit::Stop };
                if matches!(cmd, Command::Connect) {
                    continue;
                }
                let quitting = matches!(cmd, Command::Disconnect { .. });
                let out = session.command(cmd);
                flush(out, session);
                if quitting {
                    // Close the queue so the writer exits once QUIT is on the wire, then
                    // give the server a moment to close. Both waits are bounded.
                    // Incoming lines are still handled so replies and echoes are not lost.
                    drop(queue);
                    let mut deadline = tokio::time::Instant::now() + QUIT_DRAIN;
                    let mut written = false;
                    loop {
                        tokio::select! {
                            _ = &mut writer_task, if !written => {
                                written = true;
                                deadline = deadline.min(tokio::time::Instant::now() + Duration::from_secs(2));
                            }
                            line = reader.next_line() => {
                                let Ok(Some(line)) = line else { break };
                                let Ok(msg) = Message::parse(&line) else { continue };
                                for e in session.handle(&msg).events {
                                    let _ = events.send(e);
                                }
                            }
                            _ = tokio::time::sleep_until(deadline) => break,
                        }
                    }
                    *shared.write().unwrap() = session.state.clone();
                    return Exit::Stop;
                }
            }
        }
    }
}
