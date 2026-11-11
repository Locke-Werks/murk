//! Transport: TCP or TLS, line framing, and outgoing flood control.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadHalf, WriteHalf,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore};

pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

pub type BoxStream = Box<dyn Stream>;

pub async fn connect(host: &str, port: u16, tls: bool, insecure: bool) -> io::Result<BoxStream> {
    let tcp = TcpStream::connect((host, port)).await?;
    tcp.set_nodelay(true)?;
    if !tls {
        return Ok(Box::new(tcp));
    }
    let config = tls_config(insecure);
    let name = rustls_pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let stream = TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await?;
    Ok(Box::new(stream))
}

fn tls_config(insecure: bool) -> ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions");
    if insecure {
        return builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify(provider)))
            .with_no_client_auth();
    }
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    builder.with_root_certificates(roots).with_no_client_auth()
}

#[derive(Debug)]
struct NoVerify(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &rustls_pki_types::CertificateDer<'_>,
        _: &[rustls_pki_types::CertificateDer<'_>],
        _: &rustls_pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls_pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls_pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub struct LineReader {
    inner: BufReader<ReadHalf<BoxStream>>,
    buf: Vec<u8>,
}

impl LineReader {
    /// Next line, decoded as UTF-8 with a Latin-1 fallback for legacy networks.
    pub async fn next_line(&mut self) -> io::Result<Option<String>> {
        loop {
            self.buf.clear();
            let n = self.inner.read_until(b'\n', &mut self.buf).await?;
            if n == 0 {
                return Ok(None);
            }
            while matches!(self.buf.last(), Some(b'\n' | b'\r')) {
                self.buf.pop();
            }
            if self.buf.is_empty() {
                continue;
            }
            let line = match std::str::from_utf8(&self.buf) {
                Ok(s) => s.to_string(),
                Err(_) => self.buf.iter().map(|&b| b as char).collect(),
            };
            return Ok(Some(line));
        }
    }
}

/// Raw writer; throttling lives in the writer task.
pub struct LineWriter {
    inner: WriteHalf<BoxStream>,
}

impl LineWriter {
    async fn write_line(&mut self, line: &str) -> io::Result<()> {
        let mut bytes = Vec::with_capacity(line.len() + 2);
        bytes.extend_from_slice(line.as_bytes());
        bytes.extend_from_slice(b"\r\n");
        self.inner.write_all(&bytes).await?;
        self.inner.flush().await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Sent immediately, ahead of the queue and outside the throttle: PONG.
    /// QUIT is deliberately not urgent, so lines typed before it still go out.
    Urgent,
    Normal,
    /// Dropped when anything else is waiting: typing notifications.
    Low,
}

impl Priority {
    pub fn of(m: &murk_proto::Message) -> Priority {
        match m.command.as_str() {
            "PONG" => Priority::Urgent,
            "TAGMSG" if m.has_tag("+typing") => Priority::Low,
            _ => Priority::Normal,
        }
    }
}

/// Each throttled line costs `PENALTY`; the send clock may run at most `BURST` ahead of real time.
const PENALTY: Duration = Duration::from_millis(2000);
const BURST: Duration = Duration::from_secs(10);

pub type WriteQueue = tokio::sync::mpsc::UnboundedSender<(Priority, String)>;

/// Run the writer on its own task so reading never waits on the throttle.
/// The task ends when every sender is dropped, or with the first write error.
pub fn spawn_writer(mut w: LineWriter) -> (WriteQueue, tokio::task::JoinHandle<io::Result<()>>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(Priority, String)>();
    let task = tokio::spawn(async move {
        let mut queue: std::collections::VecDeque<(Priority, String)> = Default::default();
        let mut clock = Instant::now();
        let enqueue = |q: &mut std::collections::VecDeque<(Priority, String)>,
                       item: (Priority, String)| {
            if item.0 == Priority::Urgent {
                let pos = q
                    .iter()
                    .position(|(p, _)| *p != Priority::Urgent)
                    .unwrap_or(q.len());
                q.insert(pos, item);
            } else {
                q.push_back(item);
            }
        };
        loop {
            while let Ok(item) = rx.try_recv() {
                enqueue(&mut queue, item);
            }
            let Some((prio, line)) = queue.pop_front() else {
                match rx.recv().await {
                    Some(item) => enqueue(&mut queue, item),
                    None => return Ok(()),
                }
                continue;
            };
            if prio == Priority::Low && !queue.is_empty() {
                continue;
            }
            let now = Instant::now();
            clock = clock.max(now);
            if prio != Priority::Urgent && clock > now + BURST {
                let wait = clock - now - BURST;
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    item = rx.recv() => {
                        if let Some(item) = item {
                            // Re-evaluate: the new item may be urgent.
                            queue.push_front((prio, line));
                            enqueue(&mut queue, item);
                            continue;
                        }
                        // All senders gone: drain the rest at the normal pace.
                        tokio::time::sleep(wait).await;
                    }
                }
            }
            clock += PENALTY;
            w.write_line(&line).await?;
        }
    });
    (tx, task)
}

pub fn split(stream: BoxStream) -> (LineReader, LineWriter) {
    let (r, w) = tokio::io::split(stream);
    (
        LineReader {
            inner: BufReader::new(r),
            buf: Vec::with_capacity(1024),
        },
        LineWriter { inner: w },
    )
}
