//! Link previews. Every fetch is bounded in size and time, images are cached on
//! disk by URL hash, and HTML pages are reduced to their OpenGraph card.
//!
//! Fetching a link tells its host your address, so frontends make this opt-in
//! (on by default, with a setting to turn it off).

use reqwest::header::{CONTENT_TYPE, USER_AGENT};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;

const MAX_IMAGE: usize = 12 * 1024 * 1024;
const MAX_HTML: usize = 512 * 1024;
pub mod upload;

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Preview {
    pub url: String,
    /// "image" for a direct image, "card" for a page with OpenGraph data.
    pub kind: String,
    pub title: String,
    pub description: String,
    pub site: String,
    /// Local file holding the image or the card's thumbnail, if any.
    pub image: Option<PathBuf>,
}

#[derive(Clone)]
pub struct Previewer {
    client: reqwest::Client,
    cache_dir: PathBuf,
    memo: Arc<Mutex<HashMap<String, Option<Preview>>>>,
}

fn install_crypto() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl Previewer {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self, String> {
        install_crypto();
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| e.to_string())?;
        let cache_dir = cache_dir.into();
        let _ = std::fs::create_dir_all(&cache_dir);
        Ok(Previewer {
            client,
            cache_dir,
            memo: Arc::default(),
        })
    }

    /// Preview for `url`, or `None` when there is nothing worth showing. Results,
    /// failures included, are remembered for the life of the process.
    pub async fn preview(&self, url: &str) -> Option<Preview> {
        if let Some(p) = self.memo.lock().await.get(url) {
            return p.clone();
        }
        let p = self.fetch(url).await.ok().flatten();
        self.memo.lock().await.insert(url.to_string(), p.clone());
        p
    }

    async fn fetch(&self, url: &str) -> Result<Option<Preview>, String> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Ok(None);
        }
        if let Some(id) = youtube_id(url) {
            let thumb = format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg");
            let image = self.image(&thumb).await.ok();
            let mut p = self.page(url).await.ok().flatten().unwrap_or_default();
            p.url = url.into();
            p.kind = "video".into();
            p.site = "YouTube".into();
            p.image = image;
            return Ok(Some(p));
        }
        let resp = self.get(url).await?;
        let ctype = content_type(&resp);
        if ctype.starts_with("image/") {
            let path = self.store(url, resp, MAX_IMAGE).await?;
            return Ok(Some(Preview {
                url: url.into(),
                kind: "image".into(),
                image: Some(path),
                ..Default::default()
            }));
        }
        // Direct video and audio play inline from the URL; nothing is downloaded here.
        if ctype.starts_with("video/") || ctype.starts_with("audio/") {
            drop(resp);
            let kind = if ctype.starts_with("video/") {
                "movie"
            } else {
                "audio"
            };
            let site = reqwest::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_default();
            let title = url
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .to_string();
            return Ok(Some(Preview {
                url: url.into(),
                kind: kind.into(),
                title,
                site,
                ..Default::default()
            }));
        }
        if ctype.starts_with("text/html") || ctype.starts_with("application/xhtml") {
            let html = read_capped(resp, MAX_HTML).await?;
            return Ok(self.card(url, &String::from_utf8_lossy(&html)).await);
        }
        Ok(None)
    }

    async fn page(&self, url: &str) -> Result<Option<Preview>, String> {
        let resp = self.get(url).await?;
        let html = read_capped(resp, MAX_HTML).await?;
        Ok(self.card(url, &String::from_utf8_lossy(&html)).await)
    }

    async fn card(&self, url: &str, html: &str) -> Option<Preview> {
        let meta = parse_meta(html);
        let get = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| meta.get(*k).filter(|v| !v.is_empty()).cloned())
                .unwrap_or_default()
        };
        let mut title = get(&["og:title", "twitter:title"]);
        if title.is_empty() {
            title = html_title(html).unwrap_or_default();
        }
        let description = get(&["og:description", "twitter:description", "description"]);
        if title.is_empty() && description.is_empty() {
            return None;
        }
        let site = {
            let s = get(&["og:site_name"]);
            if s.is_empty() {
                host(url).to_string()
            } else {
                s
            }
        };
        let img_url = get(&["og:image", "og:image:url", "twitter:image"]);
        let image = if img_url.is_empty() {
            None
        } else {
            self.image(&absolute(url, &img_url)).await.ok()
        };
        Some(Preview {
            url: url.into(),
            kind: "card".into(),
            title: truncate(&title, 200),
            description: truncate(&description, 400),
            site,
            image,
        })
    }

    async fn image(&self, url: &str) -> Result<PathBuf, String> {
        let path = self.cache_path(url);
        if path.exists() {
            return Ok(path);
        }
        let resp = self.get(url).await?;
        if !content_type(&resp).starts_with("image/") {
            return Err("not an image".into());
        }
        self.store(url, resp, MAX_IMAGE).await
    }

    async fn get(&self, url: &str) -> Result<reqwest::Response, String> {
        let resp = self
            .client
            .get(url)
            .header(
                USER_AGENT,
                concat!(
                    "Murk/",
                    env!("CARGO_PKG_VERSION"),
                    " (IRC client link preview)"
                ),
            )
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        Ok(resp)
    }

    async fn store(
        &self,
        url: &str,
        resp: reqwest::Response,
        cap: usize,
    ) -> Result<PathBuf, String> {
        let bytes = read_capped(resp, cap).await?;
        let path = self.cache_path(url);
        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, &bytes)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(path)
    }

    fn cache_path(&self, url: &str) -> PathBuf {
        // FNV-1a 64 is plenty to name cache files; collisions only cost a wrong thumbnail.
        let h = url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
        });
        self.cache_dir.join(format!("{h:016x}.img"))
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    if resp.content_length().is_some_and(|n| n as usize > cap) {
        return Err("too large".into());
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        out.extend_from_slice(&chunk);
        if out.len() > cap {
            // HTML only needs its head; images over the cap are refused.
            if cap == MAX_HTML {
                break;
            }
            return Err("too large".into());
        }
    }
    Ok(out)
}

pub fn youtube_id(url: &str) -> Option<&str> {
    let rest = url
        .split_once("youtube.com/watch?v=")
        .or_else(|| url.split_once("youtu.be/"))
        .or_else(|| url.split_once("youtube.com/shorts/"))?
        .1;
    let id = &rest[..rest.find(['&', '?', '#', '/']).unwrap_or(rest.len())];
    (id.len() == 11).then_some(id)
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let h = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    h.strip_prefix("www.").unwrap_or(h)
}

fn absolute(base: &str, link: &str) -> String {
    if link.starts_with("http://") || link.starts_with("https://") {
        return link.into();
    }
    let scheme = if base.starts_with("http://") {
        "http"
    } else {
        "https"
    };
    if let Some(rest) = link.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    let rest = base.split_once("://").map(|(_, r)| r).unwrap_or(base);
    let authority = &rest[..rest.find('/').unwrap_or(rest.len())];
    if link.starts_with('/') {
        format!("{scheme}://{authority}{link}")
    } else {
        format!("{scheme}://{authority}/{link}")
    }
}

/// `<meta property|name=... content=...>` pairs, keys lowercased, entities decoded.
fn parse_meta(html: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(i) = lower[pos..].find("<meta") {
        let start = pos + i;
        let end = lower[start..]
            .find('>')
            .map(|e| start + e)
            .unwrap_or(lower.len());
        let tag = &html[start..end];
        let key = attr(tag, "property").or_else(|| attr(tag, "name"));
        if let (Some(k), Some(v)) = (key, attr(tag, "content")) {
            out.entry(k.to_ascii_lowercase())
                .or_insert_with(|| decode(&v));
        }
        pos = end;
        if lower[pos..].starts_with("</head") || pos >= lower.len() {
            break;
        }
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // Must be a whole attribute name.
        if at > 0 && !lower.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = lower[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let offset = tag.len() - rest.len();
        let value = tag[offset..].trim_start();
        let q = value.chars().next()?;
        if q == '"' || q == '\'' {
            let v = &value[1..];
            return v.find(q).map(|e| v[..e].to_string());
        }
        let e = value
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(value.len());
        return Some(value[..e].to_string());
    }
    None
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let s = lower.find("<title")?;
    let s = s + lower[s..].find('>')? + 1;
    let e = s + lower[s..].find("</title")?;
    Some(decode(html[s..e].trim()))
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest[..rest.len().min(10)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16)
                .ok()
                .and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        return s;
    }
    let mut t: String = s.chars().take(n).collect();
    t.push('\u{2026}');
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opengraph() {
        let html = r#"<html><head><title>Fallback</title>
            <meta property="og:title" content="Murk &amp; friends">
            <meta name='description' content='An IRC client'>
            <meta property=og:image content="/img/card.png">
            </head><body>"#;
        let m = parse_meta(html);
        assert_eq!(m["og:title"], "Murk & friends");
        assert_eq!(m["description"], "An IRC client");
        assert_eq!(m["og:image"], "/img/card.png");
        assert_eq!(html_title(html).as_deref(), Some("Fallback"));
        assert_eq!(
            absolute("https://ex.com/a/b", "/img/card.png"),
            "https://ex.com/img/card.png"
        );
        assert_eq!(
            absolute("https://ex.com/a", "//cdn.ex/x.png"),
            "https://cdn.ex/x.png"
        );
    }

    #[test]
    fn youtube() {
        assert_eq!(
            youtube_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1"),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(
            youtube_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(youtube_id("https://example.com/"), None);
    }

    #[test]
    fn hosts_and_entities() {
        assert_eq!(host("https://www.example.com:443/x"), "example.com:443");
        assert_eq!(decode("a &#x41; &#66; &bogus; &"), "a A B &bogus; &");
    }
}
