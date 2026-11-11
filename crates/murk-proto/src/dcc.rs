//! DCC offers carried in CTCP: `DCC SEND <file> <ip> <port> [size] [token]`
//! and `DCC CHAT chat <ip> <port>`.

use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DccKind {
    Send,
    Chat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DccOffer {
    pub kind: DccKind,
    /// As sent; never use it as a path without sanitising.
    pub filename: String,
    pub ip: IpAddr,
    /// Zero for a passive (reverse) offer: the sender wants us to listen.
    pub port: u16,
    pub size: Option<u64>,
    /// Present on passive offers and their replies.
    pub token: Option<String>,
}

impl DccOffer {
    pub fn passive(&self) -> bool {
        self.port == 0 && self.token.is_some()
    }
}

/// Parse the arguments after `DCC` in a CTCP message.
pub fn parse(args: &str) -> Option<DccOffer> {
    let (kind, rest) = args.trim().split_once(' ')?;
    let kind = match kind.to_ascii_uppercase().as_str() {
        "SEND" => DccKind::Send,
        "CHAT" => DccKind::Chat,
        _ => return None,
    };
    let rest = rest.trim_start();
    // The filename may be quoted to carry spaces.
    let (filename, rest) = if let Some(q) = rest.strip_prefix('"') {
        let end = q.find('"')?;
        (q[..end].to_string(), q[end + 1..].trim_start())
    } else {
        let (f, r) = rest.split_once(' ')?;
        (f.to_string(), r)
    };
    let mut parts = rest.split_whitespace();
    let ip = parse_ip(parts.next()?)?;
    let port: u16 = parts.next()?.parse().ok()?;
    let size = parts.next().and_then(|s| s.parse().ok());
    let token = parts.next().map(str::to_string);
    Some(DccOffer {
        kind,
        filename,
        ip,
        port,
        size,
        token,
    })
}

/// Classic DCC sends IPv4 as one decimal integer; newer clients send text.
fn parse_ip(s: &str) -> Option<IpAddr> {
    if let Ok(n) = s.parse::<u32>() {
        return Some(IpAddr::V4(Ipv4Addr::from(n)));
    }
    s.parse().ok()
}

/// Format an offer the way peers expect: IPv4 as an integer, names quoted when needed.
pub fn encode_send(
    filename: &str,
    ip: IpAddr,
    port: u16,
    size: u64,
    token: Option<&str>,
) -> String {
    let ip = match ip {
        IpAddr::V4(v4) => u32::from(v4).to_string(),
        IpAddr::V6(v6) => v6.to_string(),
    };
    let name = if filename.contains(' ') {
        format!("\"{}\"", filename.replace('"', "'"))
    } else {
        filename.to_string()
    };
    match token {
        Some(t) => format!("SEND {name} {ip} {port} {size} {t}"),
        None => format!("SEND {name} {ip} {port} {size}"),
    }
}

/// A filename safe to create in the downloads folder: no directories, no
/// reserved names, nothing hidden, never empty.
pub fn safe_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut s: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    s = s
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .to_string();
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if s.is_empty() || reserved {
        s = format!("download_{s}");
    }
    if s.len() > 200 {
        s = s.chars().take(200).collect();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_offers() {
        let o = parse("SEND notes.txt 3232235777 5000 1234").unwrap();
        assert_eq!(o.ip, "192.168.1.1".parse::<IpAddr>().unwrap());
        assert_eq!((o.port, o.size), (5000, Some(1234)));
        assert!(!o.passive());

        let o = parse("SEND \"my file.zip\" 10.0.0.2 0 99 tok1").unwrap();
        assert_eq!(o.filename, "my file.zip");
        assert!(o.passive());
        assert_eq!(o.token.as_deref(), Some("tok1"));

        let o = parse("CHAT chat ::1 4000").unwrap();
        assert_eq!(o.kind, DccKind::Chat);
        assert!(parse("RESUME x 1 2").is_none());
    }

    #[test]
    fn roundtrip_encoding() {
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        let s = encode_send("a b.txt", ip, 4000, 10, None);
        assert_eq!(s, "SEND \"a b.txt\" 3232235777 4000 10");
        assert_eq!(parse(&s).unwrap().filename, "a b.txt");
    }

    #[test]
    fn sanitises_names() {
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename("C:\\Windows\\evil.exe"), "evil.exe");
        assert_eq!(safe_filename(".bashrc"), "bashrc");
        assert_eq!(safe_filename("CON.txt"), "download_CON.txt");
        assert_eq!(safe_filename("a<b>?.png"), "a_b__.png");
        assert_eq!(safe_filename(""), "download_");
    }
}
