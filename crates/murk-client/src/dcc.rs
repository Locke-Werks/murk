//! DCC transfers as the client sees them: offers waiting for a yes, transfers
//! in flight, and finished ones. Network work runs on tokio tasks that report
//! back over a channel the client drains on every tick.

use murk_core::{Command, Engine, NetworkId};
use murk_proto::dcc::{DccControl, DccControlKind, DccKind, DccOffer};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// How long we wait for the sender to answer a RESUME before starting over.
pub const RESUME_TIMEOUT: Duration = Duration::from_secs(30);

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

/// What an outgoing transfer's task and a later RESUME need to agree on.
#[derive(Debug, Default)]
struct Outgoing {
    /// Zero until the task is listening.
    port: AtomicU32,
    /// Where to start once the peer connects; moved by an accepted RESUME.
    offset: AtomicU64,
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
    sending: Option<Arc<Outgoing>>,
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
    /// A resume was agreed; progress starts from here.
    Resumed(u64),
    /// The resume fell through and the download restarted at a new path.
    Restarted(PathBuf),
}

/// A RESUME we sent, waiting for the sender's ACCEPT.
struct PendingResume {
    network: NetworkId,
    peer: String,
    port: u16,
    token: Option<String>,
    reply: oneshot::Sender<u64>,
}

pub struct Transfers {
    pub list: Vec<Transfer>,
    next: u32,
    tx: mpsc::Sender<(u32, Update)>,
    rx: mpsc::Receiver<(u32, Update)>,
    resumes: HashMap<u32, PendingResume>,
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
            resumes: HashMap::new(),
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

/// Where an offered download can pick up. Only our own `.part` from an
/// earlier attempt qualifies: a shorter file under the final name may be an
/// unrelated file of the user's, and appending to it would destroy it. Only a
/// known size makes "shorter" meaningful, and a final file alongside a
/// partial is ambiguous, so neither case resumes. The bool is always false
/// now and kept so callers need not change shape.
fn resume_point(whole: Option<u64>, part: Option<u64>, size: Option<u64>) -> Option<(u64, bool)> {
    let size = size.filter(|&s| s > 0)?;
    let usable = |n: u64| n > 0 && n < size;
    match (whole, part) {
        (None, Some(p)) if usable(p) => Some((p, false)),
        _ => None,
    }
}

fn file_len(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

/// Whether a RESUME or ACCEPT names this transfer: by token when passive
/// (port 0), otherwise by port.
fn control_matches(c: &DccControl, port: u16, token: Option<&str>) -> bool {
    if c.port == 0 {
        c.token.is_some() && c.token.as_deref() == token
    } else {
        c.port == port
    }
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
            sending: None,
        }))
    }

    /// Accept an offer and start downloading into the downloads folder. When
    /// a shorter copy of the file is already there, ask the sender to resume.
    pub fn accept(&mut self, id: u32, engine: &Engine) -> Result<PathBuf, String> {
        let reporter = self.reporter(id);
        let tx = self.tx.clone();
        // A path another live transfer is writing must not be resumed into:
        // two writers on one `.part` corrupt it.
        let busy: Vec<PathBuf> = self
            .list
            .iter()
            .filter(|t| t.id != id && matches!(t.state, State::Active | State::Offered))
            .filter_map(|t| t.path.clone())
            .collect();
        let t = self
            .list
            .iter_mut()
            .find(|t| t.id == id && t.state == State::Offered)
            .ok_or("No such pending offer")?;
        let offer = t.offer.clone().ok_or("No offer")?;
        let dir = download_dir();
        let whole = dir.join(&t.filename);
        let part = murk_dcc::part_path(&whole);
        let resume = if busy.contains(&whole) {
            None
        } else {
            resume_point(file_len(&whole), file_len(&part), offer.size)
        };
        let dest = match resume {
            Some(_) => whole.clone(),
            None => murk_dcc::unique_path(&dir, &t.filename),
        };
        t.path = Some(dest.clone());
        t.state = State::Active;
        t.started = Some(Instant::now());
        let (net, peer, name, size) = (
            t.network,
            t.peer.clone(),
            offer.filename.clone(),
            offer.size,
        );
        let accepted = resume.map(|(position, _)| {
            let (reply, rx) = oneshot::channel();
            self.resumes.insert(
                id,
                PendingResume {
                    network: net,
                    peer: peer.clone(),
                    port: offer.port,
                    token: offer.token.clone(),
                    reply,
                },
            );
            let body = murk_proto::dcc::encode_control(&DccControl {
                kind: DccControlKind::Resume,
                filename: name.clone(),
                port: offer.port,
                position,
                token: offer.token.clone(),
            });
            ctcp_dcc(engine, net, &peer, &body);
            rx
        });
        let engine = engine.clone();
        let local_name = self.list.iter().find(|t| t.id == id).unwrap().filename.clone();
        let dest2 = dest.clone();
        tokio::spawn(async move {
            let _ = tx.send((id, Update::Active));
            let mut dest = dest2;
            let mut offset = 0;
            if let (Some((ours, adopt)), Some(rx)) = (resume, accepted) {
                match tokio::time::timeout(RESUME_TIMEOUT, rx).await {
                    Ok(Ok(pos)) if pos <= ours => {
                        let ready = if adopt {
                            std::fs::rename(&whole, &part)
                        } else {
                            Ok(())
                        };
                        if let Err(e) = ready {
                            let _ = tx.send((id, Update::Failed(e.to_string())));
                            return;
                        }
                        offset = pos;
                        let _ = tx.send((id, Update::Resumed(pos)));
                    }
                    Ok(Ok(pos)) => {
                        let _ = tx.send((
                            id,
                            Update::Failed(format!(
                                "sender wants to resume at byte {pos}, but only {ours} are here"
                            )),
                        ));
                        return;
                    }
                    // No ACCEPT: the sender does not resume. Take the whole file
                    // under a fresh name so the partial survives for another try.
                    _ => {
                        dest = murk_dcc::unique_path(&dir, &local_name);
                        let _ = tx.send((id, Update::Restarted(dest.clone())));
                    }
                }
            }
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
                            Ok(Ok((s, _))) => {
                                murk_dcc::receive_on(s, size, &dest, offset, reporter).await
                            }
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
                    &dest,
                    offset,
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
        let shared = Arc::new(Outgoing::default());
        let shared2 = shared.clone();
        let (nick2, name2, path2) = (nick.to_string(), name.clone(), path.clone());
        tokio::spawn(async move {
            let r = match murk_dcc::listen().await {
                Ok((l, port)) => {
                    // Recorded before the offer goes out, so a RESUME can never beat it.
                    shared2.port.store(port as u32, Ordering::SeqCst);
                    let body = murk_proto::dcc::encode_send(
                        &name2,
                        murk_dcc::local_ip(),
                        port,
                        size,
                        None,
                    );
                    ctcp_dcc(&engine, network, &nick2, &body);
                    match murk_dcc::accept_peer(l).await {
                        Ok(s) => {
                            let _ = tx.send((id, Update::Active));
                            let offset = shared2.offset.load(Ordering::SeqCst);
                            murk_dcc::send_stream(s, &path2, offset, reporter).await
                        }
                        Err(e) => Err(e),
                    }
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
            sending: Some(shared),
        }))
    }

    /// Handle a DCC RESUME (we are sending) or ACCEPT (we asked to resume).
    /// Returns a line for the user when there is something to say.
    pub fn control(
        &mut self,
        network: NetworkId,
        from: &str,
        c: DccControl,
        engine: &Engine,
    ) -> Option<String> {
        match c.kind {
            DccControlKind::Resume => {
                let t = self.list.iter_mut().find(|t| {
                    t.network == network
                        && t.state == State::Waiting
                        && t.peer.eq_ignore_ascii_case(from)
                        && t.sending.as_ref().is_some_and(|s| {
                            let port = s.port.load(Ordering::SeqCst) as u16;
                            port != 0 && control_matches(&c, port, None)
                        })
                });
                let Some(t) = t else {
                    return Some(format!(
                        "Ignored DCC RESUME from {from} for {}: no matching offer",
                        c.filename
                    ));
                };
                let size = t.size.unwrap_or(0);
                if c.position > size {
                    return Some(format!(
                        "Ignored DCC RESUME from {from}: byte {} is past the end of {}",
                        c.position, t.filename
                    ));
                }
                t.sending
                    .as_ref()
                    .unwrap()
                    .offset
                    .store(c.position, Ordering::SeqCst);
                t.done = c.position;
                self.rev += 1;
                let msg = format!(
                    "{from} resumes {} from {}",
                    t.filename,
                    human_size(c.position)
                );
                // Echo their filename and token back verbatim: some clients match on them.
                let body = murk_proto::dcc::encode_control(&DccControl {
                    kind: DccControlKind::Accept,
                    ..c
                });
                ctcp_dcc(engine, network, from, &body);
                Some(msg)
            }
            DccControlKind::Accept => {
                let id = self.resumes.iter().find_map(|(id, p)| {
                    (p.network == network
                        && p.peer.eq_ignore_ascii_case(from)
                        && control_matches(&c, p.port, p.token.as_deref()))
                    .then_some(*id)
                })?;
                let p = self.resumes.remove(&id)?;
                // A closed receiver means the wait already timed out and fell back.
                let _ = p.reply.send(c.position);
                None
            }
        }
    }

    /// Apply queued updates. Returns (id, message) for anything worth telling
    /// the user: finished transfers, and resumes that took or fell through.
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
                Update::Resumed(n) => {
                    t.done = n;
                    finished.push((
                        id,
                        format!("Resuming {} from {}", t.filename, human_size(n)),
                    ));
                }
                Update::Restarted(p) => {
                    t.done = 0;
                    finished.push((
                        id,
                        format!(
                            "{} did not agree to resume {}; downloading it whole to {}",
                            t.peer,
                            t.filename,
                            p.display()
                        ),
                    ));
                    t.path = Some(p);
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
            if t.finished() {
                self.resumes.remove(&id);
            }
            if let Some(s) = t.started {
                let secs = s.elapsed().as_secs_f64().max(0.001);
                t.rate = t.done as f64 / secs;
            }
        }
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_point_rules() {
        // Our own partial resumes where it stopped.
        assert_eq!(resume_point(None, Some(400), Some(1000)), Some((400, false)));
        // A short file of the final name is adopted as the partial.
        assert_eq!(resume_point(Some(400), None, Some(1000)), None);
        // Complete, oversized, empty, unknown size, or ambiguous: start fresh.
        assert_eq!(resume_point(Some(1000), None, Some(1000)), None);
        assert_eq!(resume_point(None, Some(1200), Some(1000)), None);
        assert_eq!(resume_point(None, Some(0), Some(1000)), None);
        assert_eq!(resume_point(None, Some(400), None), None);
        assert_eq!(resume_point(None, Some(400), Some(0)), None);
        assert_eq!(resume_point(Some(100), Some(400), Some(1000)), None);
        assert_eq!(resume_point(None, None, Some(1000)), None);
        // Past 4 GiB the offset is still exact; only the acks wrap.
        let big = (5u64 << 30) + 3;
        assert_eq!(resume_point(None, Some(big), Some(6 << 30)), Some((big, false)));
    }

    #[test]
    fn matching_by_port_or_token() {
        let mut c = murk_proto::dcc::parse_control("RESUME f 5000 10").unwrap();
        assert!(control_matches(&c, 5000, None));
        assert!(!control_matches(&c, 5001, None));
        c = murk_proto::dcc::parse_control("ACCEPT f 0 10 tok").unwrap();
        assert!(control_matches(&c, 0, Some("tok")));
        assert!(!control_matches(&c, 0, Some("other")));
        assert!(!control_matches(&c, 0, None));
        // Port 0 without a token identifies nothing.
        c = murk_proto::dcc::parse_control("ACCEPT f 0 10").unwrap();
        assert!(!control_matches(&c, 0, None));
    }
}
