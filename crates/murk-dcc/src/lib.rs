//! DCC SEND transfers over plain TCP, with the classic 32-bit acknowledgements.
//!
//! The receiver acknowledges each chunk with the total received so far as a
//! big-endian u32 (wrapping past 4 GiB); the sender may read or ignore them.

use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const CHUNK: usize = 64 * 1024;
/// How long a listening sender waits for the peer to connect.
pub const ACCEPT_TIMEOUT: Duration = Duration::from_secs(180);
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Progress callback: bytes transferred so far.
pub type Progress = Box<dyn Fn(u64) + Send + Sync>;

/// The address peers on the local network can reach us at. Behind NAT this is
/// a private address; users on the open internet need port forwarding.
pub fn local_ip() -> IpAddr {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
}

/// A path in `dir` for `name` that does not exist yet: `file.txt`, `file (1).txt`, ...
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Connect to a sender and save the file at `dest`. Stops at `size` when known,
/// otherwise when the sender closes. Returns the byte count.
pub async fn receive(
    addr: SocketAddr,
    size: Option<u64>,
    dest: &Path,
    progress: Progress,
) -> io::Result<u64> {
    let stream = tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timed out"))??;
    receive_on(stream, size, dest, progress).await
}

/// Receive over an already connected stream (used for passive offers, where we listened).
pub async fn receive_on(
    mut stream: TcpStream,
    size: Option<u64>,
    dest: &Path,
    progress: Progress,
) -> io::Result<u64> {
    stream.set_nodelay(true)?;
    let part = dest.with_extension(match dest.extension() {
        Some(e) => format!("{}.part", e.to_string_lossy()),
        None => "part".into(),
    });
    let mut file = File::create(&part).await?;
    let mut buf = vec![0u8; CHUNK];
    let mut total: u64 = 0;
    let result: io::Result<()> = async {
        loop {
            if size.is_some_and(|s| total >= s) {
                break;
            }
            let n = tokio::time::timeout(IDLE_TIMEOUT, stream.read(&mut buf))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "transfer stalled"))??;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).await?;
            total += n as u64;
            stream.write_all(&(total as u32).to_be_bytes()).await?;
            progress(total);
        }
        file.flush().await?;
        Ok(())
    }
    .await;
    drop(file);
    match result {
        Ok(()) if size.is_none_or(|s| total >= s) => {
            tokio::fs::rename(&part, dest).await?;
            Ok(total)
        }
        Ok(()) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "sender stopped after {total} of {} bytes",
                    size.unwrap_or(0)
                ),
            ))
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

/// Listen on an ephemeral port on every interface.
pub async fn listen() -> io::Result<(TcpListener, u16)> {
    let l = TcpListener::bind(("0.0.0.0", 0)).await?;
    let port = l.local_addr()?.port();
    Ok((l, port))
}

/// Wait for the peer on `listener`, then stream `path` to it.
pub async fn send_on(listener: TcpListener, path: &Path, progress: Progress) -> io::Result<u64> {
    let (stream, _) = tokio::time::timeout(ACCEPT_TIMEOUT, listener.accept())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "nobody connected"))??;
    send_stream(stream, path, progress).await
}

/// Connect to a peer that listened for our passive offer, then send.
pub async fn send_to(addr: SocketAddr, path: &Path, progress: Progress) -> io::Result<u64> {
    let stream = tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timed out"))??;
    send_stream(stream, path, progress).await
}

async fn send_stream(stream: TcpStream, path: &Path, progress: Progress) -> io::Result<u64> {
    stream.set_nodelay(true)?;
    let size = tokio::fs::metadata(path).await?.len();
    let mut file = File::open(path).await?;
    let (mut rd, mut wr) = stream.into_split();
    // Drain acknowledgements so the peer never blocks on a full socket buffer.
    let acks = tokio::spawn(async move {
        let mut a = [0u8; 4];
        while rd.read_exact(&mut a).await.is_ok() {}
    });
    let mut buf = vec![0u8; CHUNK];
    let mut sent: u64 = 0;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        tokio::time::timeout(IDLE_TIMEOUT, wr.write_all(&buf[..n]))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "transfer stalled"))??;
        sent += n as u64;
        progress(sent);
    }
    wr.flush().await?;
    // Give the receiver a moment to read the tail before we close.
    let _ = tokio::time::timeout(Duration::from_secs(10), acks).await;
    if sent != size {
        return Err(io::Error::other("file changed while sending"));
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[tokio::test(flavor = "multi_thread")]
    async fn loopback_transfer() {
        let dir = std::env::temp_dir().join(format!("murk-dcc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.bin");
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();

        let (listener, port) = listen().await.unwrap();
        let src2 = src.clone();
        let sender = tokio::spawn(async move { send_on(listener, &src2, Box::new(|_| {})).await });

        let seen = Arc::new(AtomicU64::new(0));
        let s2 = seen.clone();
        let dest = unique_path(&dir, "out.bin");
        let got = receive(
            SocketAddr::from(([127, 0, 0, 1], port)),
            Some(data.len() as u64),
            &dest,
            Box::new(move |n| s2.store(n, Ordering::Relaxed)),
        )
        .await
        .unwrap();
        assert_eq!(got, data.len() as u64);
        assert_eq!(sender.await.unwrap().unwrap(), data.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert_eq!(seen.load(Ordering::Relaxed), data.len() as u64);
        // A second download of the same name does not overwrite the first.
        assert_ne!(unique_path(&dir, "out.bin"), dest);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
