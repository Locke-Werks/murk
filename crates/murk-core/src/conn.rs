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

/// Token-bucket style limiter: each line costs `PENALTY`, and the send clock
/// may run at most `BURST` ahead of real time.
pub struct LineWriter {
    inner: WriteHalf<BoxStream>,
    clock: Instant,
}

const PENALTY: Duration = Duration::from_millis(2000);
const BURST: Duration = Duration::from_secs(10);

impl LineWriter {
    pub async fn send(&mut self, line: &str) -> io::Result<()> {
        let now = Instant::now();
        if self.clock < now {
            self.clock = now;
        }
        if self.clock > now + BURST {
            tokio::time::sleep(self.clock - now - BURST).await;
        }
        self.clock += PENALTY;
        let mut bytes = Vec::with_capacity(line.len() + 2);
        bytes.extend_from_slice(line.as_bytes());
        bytes.extend_from_slice(b"\r\n");
        self.inner.write_all(&bytes).await?;
        self.inner.flush().await
    }
}

pub fn split(stream: BoxStream) -> (LineReader, LineWriter) {
    let (r, w) = tokio::io::split(stream);
    (
        LineReader {
            inner: BufReader::new(r),
            buf: Vec::with_capacity(1024),
        },
        LineWriter {
            inner: w,
            clock: Instant::now(),
        },
    )
}
