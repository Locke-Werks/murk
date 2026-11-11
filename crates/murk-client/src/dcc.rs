//! DCC transfers as the client sees them: offers waiting for a yes, transfers
//! in flight, and finished ones. Network work runs on tokio tasks that report
//! back over a channel the client drains on every tick.

use murk_core::{Command, Engine, NetworkId};
use murk_proto::dcc::{DccKind, DccOffer};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Someone offered us a file; waiting for the user.
    Offered,
    /// We offered a file; waiting for the peer to connect.
    Waiting,
    Active,
    Done,
    Failed(String),
    Declined,
}

#[derive(Debug, Clone)]
pub struct Transfer {
    pub id: u32,
    pub network: NetworkId,
    pub peer: String,
    pub filename: String,
    pub size: Option<u64>,
    pub done: u64,
    pub state: State,
    pub outgoing: bool,
    /// Where the file is (sending) or will be (receiving, once accepted).
    pub path: Option<PathBuf>,
    /// Bytes per second over the transfer so far.
    pub rate: f64,
    started: Option<Instant>,
    offer: Option<DccOffer>,
}

impl Transfer {
    pub fn finished(&self) -> bool {
        matches!(self.state, State::Done | State::Failed(_) | State::Declined)
    }
}

enum Update {
    Active,
    Progress(u64),
    Done(u64),
    Failed(String),
}

pub struct Transfers {
    pub list: Vec<Transfer>,
    next: u32,
    tx: mpsc::Sender<(u32, Update)>,
    rx: mpsc::Receiver<(u32, Update)>,
    /// Bumped on any change, so views can skip identical redraws.
    pub rev: u64,
}

impl Default for Transfers {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Transfers {
            list: Vec::new(),
            next: 1,
            tx,
            rx,
            rev: 0,
        }
    }
}

pub fn download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn human_size(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", units[u])
    }
}

fn ctcp_dcc(engine: &Engine, network: NetworkId, nick: &str, body: &str) {
    engine.send(
        network,
        Command::Raw(format!("PRIVMSG {nick} :\x01DCC {body}\x01")),
    );
}

impl Transfers {
    fn push(&mut self, t: Transfer) -> u32 {
        self.rev += 1;
        self.list.push(t);
        self.list.last().unwrap().id
    }

    fn reporter(&self, id: u32) -> murk_dcc::Progress {
        let tx = self.tx.clone();
        let last = std::sync::Mutex::new(Instant::now() - Duration::from_secs(1));
        Box::new(move |n| {
            // A progress report per chunk would flood the UI; four a second is plenty.
            let mut l = last.lock().unwrap();
            if l.elapsed() >= Duration::from_millis(250) {
                *l = Instant::now();
                let _ = tx.send((id, Update::Progress(n)));
            }
        })
    }

    /// Record an incoming offer. Returns its id, or an explanation when it cannot be used.
    pub fn offered(
        &mut self,
        network: NetworkId,
        from: &str,
        offer: DccOffer,
    ) -> Result<u32, String> {
        if offer.kind == DccKind::Chat {
            return Err(format!(
                "{from} offered a DCC chat, which Murk does not support yet"
            ));
        }
        let id = self.next;
        self.next += 1;
        Ok(self.push(Transfer {
            id,
            network,
            peer: from.to_string(),
            filename: murk_proto::dcc::safe_filename(&offer.filename),
            size: offer.size,
            done: 0,
            state: State::Offered,
            outgoing: false,
            path: None,
            rate: 0.0,
            started: None,
            offer: Some(offer),
        }))
    }

    /// Accept an offer and start downloading into the downloads folder.
    pub fn accept(&mut self, id: u32, engine: &Engine) -> Result<PathBuf, String> {
        let reporter = self.reporter(id);
        let tx = self.tx.clone();
        let t = self
            .list
            .iter_mut()
            .find(|t| t.id == id && t.state == State::Offered)
            .ok_or("No such pending offer")?;
        let offer = t.offer.clone().ok_or("No offer")?;
        let dir = download_dir();
        let dest = murk_dcc::unique_path(&dir, &t.filename);
        t.path = Some(dest.clone());
        t.state = State::Active;
        t.started = Some(Instant::now());
        let (net, peer, name, size) = (
            t.network,
            t.peer.clone(),
            offer.filename.clone(),
            offer.size,
        );
        let engine = engine.clone();
        let dest2 = dest.clone();
        tokio::spawn(async move {
            let _ = tx.send((id, Update::Active));
            let r = if offer.passive() {
                // They cannot listen; we do, and tell them where.
                match murk_dcc::listen().await {
                    Ok((l, port)) => {
                        let body = murk_proto::dcc::encode_send(
                            &name,
                            murk_dcc::local_ip(),
                            port,
                            size.unwrap_or(0),
                            offer.token.as_deref(),
                        );
                        ctcp_dcc(&engine, net, &peer, &body);
                        match tokio::time::timeout(murk_dcc::ACCEPT_TIMEOUT, l.accept()).await {
                            Ok(Ok((s, _))) => murk_dcc::receive_on(s, size, &dest2, reporter).await,
                            Ok(Err(e)) => Err(e),
                            Err(_) => Err(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "peer never connected",
                            )),
                        }
                    }
                    Err(e) => Err(e),
                }
            } else {
                murk_dcc::receive(
                    SocketAddr::new(offer.ip, offer.port),
                    size,
                    &dest2,
                    reporter,
                )
                .await
            };
            let _ = tx.send((
                id,
                match r {
                    Ok(n) => Update::Done(n),
                    Err(e) => Update::Failed(e.to_string()),
                },
            ));
        });
        self.rev += 1;
        Ok(dest)
    }

    pub fn decline(&mut self, id: u32) -> bool {
        match self
            .list
            .iter_mut()
            .find(|t| t.id == id && t.state == State::Offered)
        {
            Some(t) => {
                t.state = State::Declined;
                self.rev += 1;
                true
            }
            None => false,
        }
    }

    /// Offer `path` to `nick` and wait for them to connect.
    pub fn send(
        &mut self,
        network: NetworkId,
        nick: &str,
        path: PathBuf,
        engine: &Engine,
    ) -> Result<u32, String> {
        let meta = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        let size = meta.len();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let id = self.next;
        self.next += 1;
        let reporter = self.reporter(id);
        let tx = self.tx.clone();
        let engine = engine.clone();
        let (nick2, name2, path2) = (nick.to_string(), name.clone(), path.clone());
        tokio::spawn(async move {
            let r = match murk_dcc::listen().await {
                Ok((l, port)) => {
                    let body = murk_proto::dcc::encode_send(
                        &name2,
                        murk_dcc::local_ip(),
                        port,
                        size,
                        None,
                    );
                    ctcp_dcc(&engine, network, &nick2, &body);
                    // Active once they connect; send_on reports progress from the first chunk.
                    murk_dcc::send_on(l, &path2, reporter).await
                }
                Err(e) => Err(e),
            };
            let _ = tx.send((
                id,
                match r {
                    Ok(n) => Update::Done(n),
                    Err(e) => Update::Failed(e.to_string()),
                },
            ));
        });
        Ok(self.push(Transfer {
            id,
            network,
            peer: nick.to_string(),
            filename: name,
            size: Some(size),
            done: 0,
            state: State::Waiting,
            outgoing: true,
            path: Some(path),
            rate: 0.0,
            started: Some(Instant::now()),
            offer: None,
        }))
    }

    /// Apply queued updates. Returns (id, message) for transfers that just finished.
    pub fn poll(&mut self) -> Vec<(u32, String)> {
        let mut finished = Vec::new();
        while let Ok((id, u)) = self.rx.try_recv() {
            let Some(t) = self.list.iter_mut().find(|t| t.id == id) else {
                continue;
            };
            self.rev += 1;
            match u {
                Update::Active => t.state = State::Active,
                Update::Progress(n) => {
                    t.state = State::Active;
                    t.done = n;
                }
                Update::Done(n) => {
                    t.done = n;
                    t.state = State::Done;
                    let verb = if t.outgoing { "Sent" } else { "Received" };
                    let place = t
                        .path
                        .as_ref()
                        .map(|p| format!(" ({})", p.display()))
                        .unwrap_or_default();
                    finished.push((
                        id,
                        format!(
                            "{verb} {} {} {}{place}",
                            t.filename,
                            if t.outgoing { "to" } else { "from" },
                            t.peer
                        ),
                    ));
                }
                Update::Failed(e) => {
                    t.state = State::Failed(e.clone());
                    finished.push((
                        id,
                        format!("Transfer of {} with {} failed: {e}", t.filename, t.peer),
                    ));
                }
            }
            if let Some(s) = t.started {
                let secs = s.elapsed().as_secs_f64().max(0.001);
                t.rate = t.done as f64 / secs;
            }
        }
        finished
    }
}
