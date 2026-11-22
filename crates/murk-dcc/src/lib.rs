//! DCC SEND transfers over plain TCP, with the classic 32-bit acknowledgements.
//!
//! The receiver acknowledges each chunk with the total received so far as a
//! big-endian u32 (wrapping past 4 GiB); the sender may read or ignore them.
//! A resumed transfer keeps counting from the resume offset, so acks always
//! describe the whole file, not just this connection.

use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
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

/// The acknowledgement for `total` bytes received: the low 32 bits, big-endian.
pub fn ack_bytes(total: u64) -> [u8; 4] {
    (total as u32).to_be_bytes()
}

/// Where a download collects until it is complete. Kept after a failure so a
/// later offer of the same file can resume into it.
pub fn part_path(dest: &Path) -> PathBuf {
    dest.with_extension(match dest.extension() {
        Some(e) => format!("{}.part", e.to_string_lossy()),
        None => "part".into(),
    })
}

/// A path in `dir` for `name` where neither it nor its `.part` exists yet:
/// `file.txt`, `file (1).txt`, ... Skipping partials keeps a fresh download
/// from clobbering one that could still be resumed.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let free = |p: &Path| !p.exists() && !part_path(p).exists();
    let p = dir.join(name);
    if free(&p) {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| free(p))
        .unwrap()
}

/// Connect to a sender and save the file at `dest`. Stops at `size` when known,
/// otherwise when the sender closes. With `offset` above zero, appends to the
/// existing `.part` of `dest` from that byte on (the sender must have agreed
/// via RESUME/ACCEPT). Returns the file's total size.
pub async fn receive(
    addr: SocketAddr,
    size: Option<u64>,
    dest: &Path,
    offset: u64,
    progress: Progress,
) -> io::Result<u64> {
    let stream = tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timed out"))??;
    receive_on(stream, size, dest, offset, progress).await
}

/// Receive over an already connected stream (used for passive offers, where we listened).
pub async fn receive_on(
    mut stream: TcpStream,
    size: Option<u64>,
    dest: &Path,
    offset: u64,
    progress: Progress,
) -> io::Result<u64> {
    stream.set_nodelay(true)?;
    let part = part_path(dest);
    let mut file = if offset > 0 {
        let mut f = OpenOptions::new().write(true).open(&part).await?;
        let have = f.metadata().await?.len();
        if have < offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("partial file has {have} bytes, cannot resume at {offset}"),
            ));
        }
        // Bytes past the agreed offset are the sender's to resend, not ours to keep.
        f.set_len(offset).await?;
        f.seek(io::SeekFrom::Start(offset)).await?;
        f
    } else {
        File::create(&part).await?
    };
    let mut buf = vec![0u8; CHUNK];
    let mut total: u64 = offset;
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
            stream.write_all(&ack_bytes(total)).await?;
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
            discard_if_empty(&part, total).await;
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "sender stopped after {total} of {} bytes",
                    size.unwrap_or(0)
                ),
            ))
        }
        Err(e) => {
            discard_if_empty(&part, total).await;
            Err(e)
        }
    }
}

/// A partial with data in it is worth keeping for a resume; an empty one is clutter.
async fn discard_if_empty(part: &Path, total: u64) {
    if total == 0 {
        let _ = tokio::fs::remove_file(part).await;
    }
}

/// Listen on an ephemeral port on every interface.
pub async fn listen() -> io::Result<(TcpListener, u16)> {
    let l = TcpListener::bind(("0.0.0.0", 0)).await?;
    let port = l.local_addr()?.port();
    Ok((l, port))
}

/// Wait for the peer on `listener`.
pub async fn accept_peer(listener: TcpListener) -> io::Result<TcpStream> {
    let (stream, _) = tokio::time::timeout(ACCEPT_TIMEOUT, listener.accept())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "nobody connected"))??;
    Ok(stream)
}

/// Wait for the peer on `listener`, then stream `path` to it from the start.
pub async fn send_on(listener: TcpListener, path: &Path, progress: Progress) -> io::Result<u64> {
    let stream = accept_peer(listener).await?;
    send_stream(stream, path, 0, progress).await
}

/// Connect to a peer that listened for our passive offer, then send from `offset`.
pub async fn send_to(
    addr: SocketAddr,
    path: &Path,
    offset: u64,
    progress: Progress,
) -> io::Result<u64> {
    let stream = tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timed out"))??;
    send_stream(stream, path, offset, progress).await
}

/// Stream `path` from byte `offset` (zero unless a RESUME was accepted).
/// Progress and the return value count the whole file, offset included.
pub async fn send_stream(
    stream: TcpStream,
    path: &Path,
    offset: u64,
    progress: Progress,
) -> io::Result<u64> {
    stream.set_nodelay(true)?;
    let size = tokio::fs::metadata(path).await?.len();
    if offset > size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("resume offset {offset} is past the end ({size} bytes)"),
        ));
    }
    let mut file = File::open(path).await?;
    file.seek(io::SeekFrom::Start(offset)).await?;
    let (mut rd, mut wr) = stream.into_split();
    // Drain acknowledgements so the peer never blocks on a full socket buffer.
    let acks = tokio::spawn(async move {
        let mut a = [0u8; 4];
        while rd.read_exact(&mut a).await.is_ok() {}
    });
    let mut buf = vec![0u8; CHUNK];
    let mut sent: u64 = offset;
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
            0,
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

    #[test]
    fn acks_wrap_past_four_gib() {
        assert_eq!(ack_bytes(0), [0, 0, 0, 0]);
        assert_eq!(ack_bytes(0x0102_0304), [1, 2, 3, 4]);
        // A resume at 4 GiB + 10 continues the count rather than restarting it.
        let offset = (1u64 << 32) + 10;
        assert_eq!(ack_bytes(offset + 5), 15u32.to_be_bytes());
        assert_eq!(ack_bytes(u32::MAX as u64 + 1), [0, 0, 0, 0]);
    }

    #[test]
    fn part_paths_and_unique_names() {
        let dir = std::env::temp_dir().join(format!("murk-dcc-part-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(part_path(&dir.join("a.zip")), dir.join("a.zip.part"));
        assert_eq!(part_path(&dir.join("noext")), dir.join("noext.part"));
        std::fs::write(dir.join("a.zip.part"), b"x").unwrap();
        // A lingering partial reserves its name, so a fresh download goes elsewhere.
        assert_eq!(unique_path(&dir, "a.zip"), dir.join("a (1).zip"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resumed_transfer() {
        let dir = std::env::temp_dir().join(format!("murk-dcc-resume-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.bin");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 249) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        let dest = dir.join("out.bin");
        let offset = 70_001u64;
        // Stray bytes past the offset must be dropped, not kept.
        let mut partial = data[..offset as usize].to_vec();
        partial.extend_from_slice(b"garbage");
        std::fs::write(part_path(&dest), &partial).unwrap();

        let (listener, port) = listen().await.unwrap();
        let src2 = src.clone();
        let sender = tokio::spawn(async move {
            let s = accept_peer(listener).await?;
            send_stream(s, &src2, offset, Box::new(|_| {})).await
        });
        let seen = Arc::new(AtomicU64::new(0));
        let s2 = seen.clone();
        let got = receive(
            SocketAddr::from(([127, 0, 0, 1], port)),
            Some(data.len() as u64),
            &dest,
            offset,
            Box::new(move |n| s2.store(n, Ordering::Relaxed)),
        )
        .await
        .unwrap();
        assert_eq!(got, data.len() as u64);
        assert_eq!(sender.await.unwrap().unwrap(), data.len() as u64);
        assert_eq!(seen.load(Ordering::Relaxed), data.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(!part_path(&dest).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
