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
    let (mut reader, mut writer) = conn::split(stream);
    let id = session.id;

    let flush = |out: Output, shared: &Arc<RwLock<NetworkState>>, session: &Session| {
        *shared.write().unwrap() = session.state.clone();
        for e in out.events {
            let _ = events.send(e);
        }
        out.send
    };

    let start = session.start();
    for m in flush(start, shared, session) {
        if let Err(e) = send(&mut writer, &m, id, events).await {
            return Exit::Lost(e.to_string());
        }
    }

    let mut pinged = false;
    loop {
        let timeout = if pinged { PING_TIMEOUT } else { PING_IDLE };
        tokio::select! {
            line = tokio::time::timeout(timeout, reader.next_line()) => {
                let line = match line {
                    Err(_) if pinged => return Exit::Lost("ping timeout".into()),
                    Err(_) => {
                        pinged = true;
                        let ping = Message::new("PING", vec!["murk".into()]);
                        if let Err(e) = send(&mut writer, &ping, id, events).await {
                            return Exit::Lost(e.to_string());
                        }
                        continue;
                    }
                    Ok(Ok(Some(l))) => l,
                    Ok(Ok(None)) => return Exit::Lost("connection closed".into()),
                    Ok(Err(e)) => return Exit::Lost(e.to_string()),
                };
                pinged = false;
                let Ok(msg) = Message::parse(&line) else { continue };
                let out = session.handle(&msg);
                for m in flush(out, shared, session) {
                    if let Err(e) = send(&mut writer, &m, id, events).await {
                        return Exit::Lost(e.to_string());
                    }
                }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { return Exit::Stop };
                let quitting = matches!(cmd, Command::Disconnect { .. });
                if matches!(cmd, Command::Connect) {
                    continue;
                }
                let out = session.command(cmd);
                for m in flush(out, shared, session) {
                    if let Err(e) = send(&mut writer, &m, id, events).await {
                        return Exit::Lost(e.to_string());
                    }
                }
                if quitting {
                    // Give the server a moment to close after QUIT.
                    let _ = tokio::time::timeout(Duration::from_secs(2), async {
                        while let Ok(Some(_)) = reader.next_line().await {}
                    }).await;
                    return Exit::Stop;
                }
            }
        }
    }
}

async fn send(
    writer: &mut conn::LineWriter,
    m: &Message,
    network: NetworkId,
    events: &broadcast::Sender<Event>,
) -> std::io::Result<()> {
    let _ = events.send(Event::Raw {
        network,
        msg: m.clone(),
        outgoing: true,
    });
    writer.send(&m.to_wire()).await
}
