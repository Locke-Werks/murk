//! Uploading a file to an HTTP file host and getting its public URL back.
//!
//! The request is the IRC `draft/FILEHOST` shape: a raw body with `Content-Type`
//! and `Content-Disposition`, answered by `201 Created` and a `Location` header.
//! murk-files speaks it, and so do IRC servers that advertise FILEHOST.

use futures_util::stream;
use reqwest::header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use std::path::Path;
use std::time::Duration;

/// Larger files are refused before any bytes are sent.
pub const MAX_BYTES: u64 = 500 * 1024 * 1024;
const CHUNK: usize = 256 * 1024;

/// How to authenticate to the host.
pub enum Auth {
    Bearer(String),
    Basic { user: String, pass: String },
    None,
}

/// A best guess at the type, from the extension. The host decides for itself
/// from the bytes; this only has to be close enough to send.
pub fn mime_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "bmp" => "image/bmp",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

/// Upload `path` to `endpoint`, calling `progress(sent, total)` as bytes go out.
/// Returns the file's public URL.
pub async fn upload(
    endpoint: &str,
    auth: Auth,
    path: &Path,
    progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let cannot = |e: std::io::Error| format!("cannot read {}: {e}", path.display());
    let file = tokio::fs::File::open(path).await.map_err(cannot)?;
    let total = file.metadata().await.map_err(cannot)?.len();
    check_size(total)?;
    // Read from disk as the request goes out, so a large file is never all in memory.
    let body = stream::unfold((file, 0u64, progress), move |(mut f, sent, p)| async move {
        let mut buf = vec![0u8; CHUNK];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                let sent = sent + n as u64;
                p(sent, total);
                Some((Ok::<_, std::io::Error>(buf), (f, sent, p)))
            }
            Err(e) => Some((Err(e), (f, sent, p))),
        }
    });
    send(
        endpoint,
        auth,
        reqwest::Body::wrap_stream(body),
        total,
        &file_name(path),
        mime_for(path),
    )
    .await
}

fn check_size(total: u64) -> Result<(), String> {
    if total == 0 {
        return Err("the file is empty".into());
    }
    if total > MAX_BYTES {
        return Err(format!(
            "the file is {} MB; the limit is {} MB",
            total / 1_000_000,
            MAX_BYTES / 1_000_000
        ));
    }
    Ok(())
}

/// Upload bytes already in memory, such as an image pasted from the clipboard.
pub async fn upload_bytes(
    endpoint: &str,
    auth: Auth,
    data: Vec<u8>,
    name: String,
    mime: &str,
    progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<String, String> {
    let total = data.len() as u64;
    check_size(total)?;
    // Hand the body over in chunks so progress tracks what has actually been taken.
    let chunks: Vec<Vec<u8>> = data.chunks(CHUNK).map(<[u8]>::to_vec).collect();
    let mut sent = 0u64;
    let body_stream = stream::iter(chunks.into_iter().map(move |c| {
        sent += c.len() as u64;
        progress(sent, total);
        Ok::<_, std::io::Error>(c)
    }));
    send(
        endpoint,
        auth,
        reqwest::Body::wrap_stream(body_stream),
        total,
        &name,
        mime,
    )
    .await
}

async fn send(
    endpoint: &str,
    auth: Auth,
    body: reqwest::Body,
    total: u64,
    name: &str,
    mime: &str,
) -> Result<String, String> {
    crate::install_crypto();
    // No overall timeout: 500 MB on a slow uplink can take a long time. A
    // connection that stops moving still fails on the read timeout.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let safe_name: String = name
        .chars()
        .map(|c| {
            if c == '"' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut req = client
        .post(endpoint)
        .header(CONTENT_TYPE, mime)
        .header(CONTENT_LENGTH, total)
        .header(
            CONTENT_DISPOSITION,
            format!("attachment; filename=\"{safe_name}\""),
        )
        .body(body);
    req = match auth {
        Auth::Bearer(t) => req.header(AUTHORIZATION, format!("Bearer {t}")),
        Auth::Basic { user, pass } => req.basic_auth(user, Some(pass)),
        Auth::None => req,
    };

    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            "the upload timed out".to_string()
        } else if e.is_connect() {
            "could not reach the file host".to_string()
        } else {
            format!("upload failed: {e}")
        }
    })?;
    let status = resp.status();
    let location = resp
        .headers()
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let why = text.trim();
        return Err(match status.as_u16() {
            401 | 403 => "the file host refused the upload token".into(),
            413 => "the file is too large for the host".into(),
            415 => "the host only takes images, video and audio".into(),
            _ if !why.is_empty() && why.len() < 200 => format!("upload failed: {why}"),
            _ => format!("upload failed: HTTP {}", status.as_u16()),
        });
    }
    // Location is the standard answer; murk-files also returns it in JSON.
    let url = location
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("url")?.as_str().map(str::to_string))
        })
        .ok_or("the host did not say where the file went")?;
    // A relative Location resolves against the endpoint.
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(url)
    } else {
        reqwest::Url::parse(endpoint)
            .and_then(|base| base.join(&url))
            .map(|u| u.to_string())
            .map_err(|_| "the host gave an unusable file URL".into())
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_guesses() {
        assert_eq!(mime_for(Path::new("a/B.JPG")), "image/jpeg");
        assert_eq!(mime_for(Path::new("clip.webm")), "video/webm");
        assert_eq!(mime_for(Path::new("noext")), "application/octet-stream");
    }
}
