//! Checking an IRC account login against Ergo, for FILEHOST uploads.
//!
//! A FILEHOST client sends its IRC account name and password as HTTP Basic
//! auth. Ergo's HTTP API (`POST /v1/check_auth`, loopback only) says whether
//! they are right. A good login is remembered for ten minutes so a burst of
//! uploads is one check, and an address that fails five times in ten minutes is
//! refused outright until the window passes, so this cannot be used to guess
//! passwords faster than IRC itself allows.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const REMEMBER: Duration = Duration::from_secs(600);
const FAIL_WINDOW: Duration = Duration::from_secs(600);
const FAIL_LIMIT: usize = 5;

pub struct IrcAuth {
    /// Ergo's API listener, `127.0.0.1:17713`.
    api: String,
    token: String,
    good: Mutex<HashMap<[u8; 32], Instant>>,
    failures: Mutex<HashMap<String, Vec<Instant>>>,
}

pub enum Verdict {
    Allowed,
    Denied,
    /// Too many recent failures from this address.
    Limited,
    /// Ergo could not be asked.
    Unavailable,
}

impl IrcAuth {
    pub fn new(api: String, token: String) -> Self {
        IrcAuth {
            api,
            token,
            good: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    pub async fn check(&self, user: &str, pass: &str, from: &str) -> Verdict {
        if self.limited(from) {
            return Verdict::Limited;
        }
        let key = digest(user, pass);
        let now = Instant::now();
        {
            let mut good = self.good.lock().unwrap();
            good.retain(|_, t| now.duration_since(*t) < REMEMBER);
            if good.contains_key(&key) {
                return Verdict::Allowed;
            }
        }
        match self.ask(user, pass).await {
            Some(true) => {
                self.good.lock().unwrap().insert(key, now);
                Verdict::Allowed
            }
            Some(false) => {
                self.failures
                    .lock()
                    .unwrap()
                    .entry(from.to_string())
                    .or_default()
                    .push(now);
                eprintln!("murk-files: failed IRC login for upload from {from}");
                Verdict::Denied
            }
            None => Verdict::Unavailable,
        }
    }

    fn limited(&self, from: &str) -> bool {
        let now = Instant::now();
        let mut f = self.failures.lock().unwrap();
        f.retain(|_, v| {
            v.retain(|t| now.duration_since(*t) < FAIL_WINDOW);
            !v.is_empty()
        });
        f.get(from).is_some_and(|v| v.len() >= FAIL_LIMIT)
    }

    /// One HTTP/1.1 request to Ergo on loopback. `None` when Ergo is unreachable
    /// or answers something unexpected.
    async fn ask(&self, user: &str, pass: &str) -> Option<bool> {
        let body = serde_json::json!({ "accountName": user, "passphrase": pass }).to_string();
        let request = format!(
            "POST /v1/check_auth HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.api,
            self.token,
            body.len(),
            body
        );
        let exchange = async {
            let mut stream = tokio::net::TcpStream::connect(&self.api).await.ok()?;
            stream.write_all(request.as_bytes()).await.ok()?;
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).await.ok()?;
            Some(raw)
        };
        let raw = tokio::time::timeout(Duration::from_secs(5), exchange)
            .await
            .ok()??;
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text.split_once("\r\n\r\n")?;
        if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
        v.get("success")?.as_bool()
    }
}

fn digest(user: &str, pass: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(user.to_ascii_lowercase().as_bytes());
    h.update([0]);
    h.update(pass.as_bytes());
    h.finalize().into()
}
