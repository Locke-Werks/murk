//! murk-files: a small upload host for chat.
//!
//! `POST /upload` takes a raw body plus `Content-Type` and an optional
//! `Content-Disposition: attachment; filename="..."`, and answers `201 Created`
//! with the file's URL in `Location` and in a JSON body. This is the upload
//! shape of the `soju.im/FILEHOST` IRC extension, so the same endpoint can be
//! advertised by an IRC server later.
//!
//! Only images, video and audio are accepted, and the type is decided by the
//! file's own bytes, never by what the client claimed. Files are served from
//! `/<id>/<name>` with `nosniff` and a sandboxing CSP, and deleted once older
//! than the retention period.
//!
//! Configuration is environment only:
//!   FILES_LISTEN           127.0.0.1:port (or PORT, bound on 127.0.0.1)
//!   FILES_DIR              storage directory (default $STATE_DIRECTORY)
//!   FILES_PUBLIC_URL       https://files.apps.example.com
//!   FILES_TOKENS           comma-separated bearer tokens allowed to upload
//!   FILES_RETENTION_HOURS  default 96
//!   FILES_MAX_BYTES        default 104857600 (100 MiB)

use axum::{
    Router,
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};
use subtle::ConstantTimeEq;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

struct Config {
    dir: PathBuf,
    public_url: String,
    tokens: Vec<Vec<u8>>,
    retention: Duration,
    max_bytes: u64,
}

type Shared = Arc<Config>;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn config() -> Result<(Config, String), String> {
    let listen = env("FILES_LISTEN")
        .or_else(|| env("PORT").map(|p| format!("127.0.0.1:{p}")))
        .ok_or("set FILES_LISTEN or PORT")?;
    let dir = env("FILES_DIR")
        .or_else(|| env("STATE_DIRECTORY"))
        .ok_or("set FILES_DIR (or run with StateDirectory=)")?;
    let public_url = env("FILES_PUBLIC_URL")
        .ok_or("set FILES_PUBLIC_URL")?
        .trim_end_matches('/')
        .to_string();
    let tokens: Vec<Vec<u8>> = env("FILES_TOKENS")
        .unwrap_or_default()
        .split(',')
        .map(|t| t.trim().as_bytes().to_vec())
        .filter(|t| t.len() >= 24)
        .collect();
    if tokens.is_empty() {
        return Err("set FILES_TOKENS to at least one token of 24 or more characters".into());
    }
    let hours: u64 = env("FILES_RETENTION_HOURS")
        .map(|h| {
            h.parse()
                .map_err(|_| "FILES_RETENTION_HOURS is not a number")
        })
        .transpose()?
        .unwrap_or(96);
    let max_bytes: u64 = env("FILES_MAX_BYTES")
        .map(|h| h.parse().map_err(|_| "FILES_MAX_BYTES is not a number"))
        .transpose()?
        .unwrap_or(100 * 1024 * 1024);
    Ok((
        Config {
            dir: PathBuf::from(dir),
            public_url,
            tokens,
            retention: Duration::from_secs(hours * 3600),
            max_bytes,
        },
        listen,
    ))
}

#[tokio::main]
async fn main() {
    let (cfg, listen) = match config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("murk-files: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = tokio::fs::create_dir_all(&cfg.dir).await {
        eprintln!("murk-files: cannot use {}: {e}", cfg.dir.display());
        std::process::exit(2);
    }
    let state: Shared = Arc::new(cfg);

    tokio::spawn(sweeper(state.clone()));

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/upload", post(upload))
        .route("/{id}/{name}", get(serve))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(&listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("murk-files: cannot listen on {listen}: {e}");
            std::process::exit(2);
        }
    };
    eprintln!("murk-files: listening on {listen}");
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let shutdown = async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = shutdown => {}
            _ = term.recv() => {}
        }
    };
    let _ = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;
}

fn plain(status: StatusCode, msg: &'static str) -> Response {
    (status, msg).into_response()
}

fn authorized(cfg: &Config, headers: &HeaderMap) -> bool {
    let Some(given) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    let given = given.trim().as_bytes();
    // Check every token so the time taken says nothing about which one was close.
    cfg.tokens
        .iter()
        .fold(false, |ok, t| ok | bool::from(t.as_slice().ct_eq(given)))
}

/// Types we host, decided from the file's bytes. SVG is deliberately absent:
/// it can carry script.
fn allowed(kind: &infer::Type) -> bool {
    matches!(
        kind.matcher_type(),
        infer::MatcherType::Image | infer::MatcherType::Video | infer::MatcherType::Audio
    ) && kind.mime_type() != "image/svg+xml"
}

fn new_id() -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; 12];
    getrandom::fill(&mut raw).expect("system randomness");
    raw.iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect()
}

/// A filename safe to put in a URL path and on disk: the stem the user gave,
/// reduced to a conservative alphabet, with the extension of the real type.
fn clean_name(given: Option<&str>, ext: &str) -> String {
    let stem = given
        .and_then(|n| n.rsplit(['/', '\\']).next())
        .map(|n| n.rsplit_once('.').map(|(s, _)| s).unwrap_or(n))
        .unwrap_or("");
    let mut out: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let mut out = out.trim_matches('-').to_string();
    out.truncate(60);
    if out.is_empty() {
        out = "file".into();
    }
    format!("{out}.{ext}")
}

fn filename_from(headers: &HeaderMap) -> Option<String> {
    let cd = headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()?;
    let start = cd.find("filename=")? + "filename=".len();
    let rest = &cd[start..];
    let name = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next()?
    } else {
        rest.split(';').next()?.trim()
    };
    Some(name.to_string())
}

async fn upload(State(cfg): State<Shared>, req: Request) -> Response {
    let headers = req.headers().clone();
    if !authorized(&cfg, &headers) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "a valid upload token is required",
        )
            .into_response();
    }
    if let Some(len) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        && len > cfg.max_bytes
    {
        return plain(StatusCode::PAYLOAD_TOO_LARGE, "file is too large");
    }

    let id = new_id();
    let staging = cfg.dir.join(format!(".{id}.part"));
    let mut file = match tokio::fs::File::create(&staging).await {
        Ok(f) => f,
        Err(_) => return plain(StatusCode::INTERNAL_SERVER_ERROR, "cannot store the file"),
    };

    // Stream to disk, counting, so a lying Content-Length cannot fill the disk.
    let mut head: Vec<u8> = Vec::with_capacity(8192);
    let mut total: u64 = 0;
    let mut stream = req.into_body().into_data_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            let _ = tokio::fs::remove_file(&staging).await;
            return plain(StatusCode::BAD_REQUEST, "upload interrupted");
        };
        total += chunk.len() as u64;
        if total > cfg.max_bytes {
            drop(file);
            let _ = tokio::fs::remove_file(&staging).await;
            return plain(StatusCode::PAYLOAD_TOO_LARGE, "file is too large");
        }
        if head.len() < 8192 {
            let take = (8192 - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..take]);
        }
        if file.write_all(&chunk).await.is_err() {
            let _ = tokio::fs::remove_file(&staging).await;
            return plain(StatusCode::INTERNAL_SERVER_ERROR, "cannot store the file");
        }
    }
    let _ = file.flush().await;
    drop(file);

    let kind = match infer::get(&head) {
        Some(k) if allowed(&k) => k,
        _ => {
            let _ = tokio::fs::remove_file(&staging).await;
            return plain(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "only images, video and audio are accepted",
            );
        }
    };

    let name = clean_name(filename_from(&headers).as_deref(), kind.extension());
    let dir = cfg.dir.join(&id);
    if tokio::fs::create_dir(&dir).await.is_err()
        || tokio::fs::rename(&staging, dir.join(&name)).await.is_err()
    {
        let _ = tokio::fs::remove_file(&staging).await;
        return plain(StatusCode::INTERNAL_SERVER_ERROR, "cannot store the file");
    }

    let url = format!("{}/{id}/{name}", cfg.public_url);
    let body = serde_json::json!({ "url": url, "type": kind.mime_type(), "bytes": total });
    let mut resp = (StatusCode::CREATED, body.to_string()).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if let Ok(v) = HeaderValue::from_str(&url) {
        h.insert(header::LOCATION, v);
    }
    resp
}

fn valid_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

async fn serve(
    State(cfg): State<Shared>,
    Path((id, name)): Path<(String, String)>,
    req: Request,
) -> Response {
    if !valid_id(&id) || !valid_name(&name) {
        return plain(StatusCode::NOT_FOUND, "not found");
    }
    let path = cfg.dir.join(&id).join(&name);
    let Ok(meta) = tokio::fs::metadata(&path).await else {
        return plain(StatusCode::NOT_FOUND, "not found");
    };
    // Expired files stay unreachable even if the sweeper has not run yet.
    if meta
        .modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age > cfg.retention)
    {
        return plain(StatusCode::NOT_FOUND, "not found");
    }
    // ServeFile handles Range requests, which video seeking needs.
    let mut resp = match ServeFile::new(&path).oneshot(req).await {
        Ok(r) => r.map(Body::new),
        Err(_) => return plain(StatusCode::INTERNAL_SERVER_ERROR, "cannot read the file"),
    };
    let h = resp.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; media-src 'self'; img-src 'self'; sandbox"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400, immutable"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    resp
}

/// Delete anything older than the retention period, every ten minutes.
async fn sweeper(cfg: Shared) {
    let mut tick = tokio::time::interval(Duration::from_secs(600));
    loop {
        tick.tick().await;
        let Ok(mut entries) = tokio::fs::read_dir(&cfg.dir).await else {
            continue;
        };
        let mut removed = 0usize;
        while let Ok(Some(e)) = entries.next_entry().await {
            let Ok(meta) = e.metadata().await else {
                continue;
            };
            let old = meta
                .modified()
                .ok()
                .and_then(|m| SystemTime::now().duration_since(m).ok())
                .is_some_and(|age| age > cfg.retention);
            // Half-written uploads older than an hour are abandoned.
            let stale_part = e.file_name().to_string_lossy().ends_with(".part")
                && meta
                    .modified()
                    .ok()
                    .and_then(|m| SystemTime::now().duration_since(m).ok())
                    .is_some_and(|age| age > Duration::from_secs(3600));
            if old || stale_part {
                let gone = if meta.is_dir() {
                    tokio::fs::remove_dir_all(e.path()).await
                } else {
                    tokio::fs::remove_file(e.path()).await
                };
                if gone.is_ok() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            eprintln!("murk-files: removed {removed} expired upload(s)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_and_retyped() {
        assert_eq!(clean_name(Some("My Cat (2).PNG"), "png"), "My-Cat-2.png");
        assert_eq!(clean_name(Some("../../etc/passwd"), "jpg"), "passwd.jpg");
        assert_eq!(clean_name(Some("evil.html"), "png"), "evil.png");
        assert_eq!(clean_name(None, "mp4"), "file.mp4");
        assert_eq!(clean_name(Some("..."), "gif"), "file.gif");
    }

    #[test]
    fn ids_and_names_are_validated() {
        assert!(valid_id(&new_id()));
        assert!(!valid_id("../secret"));
        assert!(valid_name("cat.png"));
        assert!(!valid_name(".part"));
        assert!(!valid_name("a/b.png"));
    }

    #[test]
    fn disposition_filename_is_read() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"holiday.jpg\""),
        );
        assert_eq!(filename_from(&h).as_deref(), Some("holiday.jpg"));
    }

    #[test]
    fn only_media_is_allowed() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert!(infer::get(&png).is_some_and(|k| allowed(&k)));
        let zip = [b'P', b'K', 3, 4, 0, 0, 0, 0];
        assert!(!infer::get(&zip).is_some_and(|k| allowed(&k)));
    }
}
