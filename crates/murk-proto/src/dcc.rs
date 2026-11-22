//! DCC offers carried in CTCP: `DCC SEND <file> <ip> <port> [size] [token]`
//! and `DCC CHAT chat <ip> <port>`, plus the resume handshake
//! `DCC RESUME|ACCEPT <file> <port> <position> [token]`.

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
    let (filename, rest) = split_filename(rest)?;
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

/// The filename may be quoted to carry spaces.
fn split_filename(rest: &str) -> Option<(String, &str)> {
    let rest = rest.trim_start();
    if let Some(q) = rest.strip_prefix('"') {
        let end = q.find('"')?;
        Some((q[..end].to_string(), q[end + 1..].trim_start()))
    } else {
        let (f, r) = rest.split_once(' ')?;
        Some((f.to_string(), r))
    }
}

fn quote_filename(filename: &str) -> String {
    if filename.contains(' ') {
        format!("\"{}\"", filename.replace('"', "'"))
    } else {
        filename.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DccControlKind {
    /// Receiver asks the sender to start from `position`.
    Resume,
    /// Sender agrees; the receiver may now connect (or listen, when passive).
    Accept,
}

/// One half of the resume handshake. Peers identify the transfer by port, or
/// by token when the offer was passive (port 0), so the filename is
/// informational: mIRC famously sends `file.ext` in place of the real name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DccControl {
    pub kind: DccControlKind,
    pub filename: String,
    pub port: u16,
    pub position: u64,
    pub token: Option<String>,
}

/// Parse `RESUME`/`ACCEPT` arguments after `DCC`. Kept apart from [`parse`]
/// because these are replies about an existing offer, never offers themselves.
pub fn parse_control(args: &str) -> Option<DccControl> {
    let (kind, rest) = args.trim().split_once(' ')?;
    let kind = match kind.to_ascii_uppercase().as_str() {
        "RESUME" => DccControlKind::Resume,
        "ACCEPT" => DccControlKind::Accept,
        _ => return None,
    };
    let (filename, rest) = split_filename(rest)?;
    let mut parts = rest.split_whitespace();
    let port: u16 = parts.next()?.parse().ok()?;
    let position: u64 = parts.next()?.parse().ok()?;
    let token = parts.next().map(str::to_string);
    Some(DccControl {
        kind,
        filename,
        port,
        position,
        token,
    })
}

pub fn encode_control(c: &DccControl) -> String {
    let verb = match c.kind {
        DccControlKind::Resume => "RESUME",
        DccControlKind::Accept => "ACCEPT",
    };
    let name = quote_filename(&c.filename);
    match &c.token {
        Some(t) => format!("{verb} {name} {} {} {t}", c.port, c.position),
        None => format!("{verb} {name} {} {}", c.port, c.position),
    }
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
    let name = quote_filename(filename);
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
        assert!(parse("ACCEPT x 1 2").is_none());
    }

    #[test]
    fn parses_resume_and_accept() {
        let c = parse_control("RESUME file.bin 5000 1024").unwrap();
        assert_eq!(c.kind, DccControlKind::Resume);
        assert_eq!((c.filename.as_str(), c.port, c.position), ("file.bin", 5000, 1024));
        assert_eq!(c.token, None);

        let c = parse_control("ACCEPT \"my file.zip\" 0 4294967296 tok1").unwrap();
        assert_eq!(c.kind, DccControlKind::Accept);
        assert_eq!(c.filename, "my file.zip");
        assert_eq!((c.port, c.position), (0, 1 << 32));
        assert_eq!(c.token.as_deref(), Some("tok1"));

        assert!(parse_control("SEND x 1 2 3").is_none());
        assert!(parse_control("RESUME x 1").is_none());
        assert!(parse_control("RESUME x 70000 1").is_none());
        assert!(parse_control("ACCEPT x 1 -5").is_none());
    }

    #[test]
    fn control_roundtrip() {
        let c = DccControl {
            kind: DccControlKind::Resume,
            filename: "a b.txt".into(),
            port: 0,
            position: 77,
            token: Some("t9".into()),
        };
        let s = encode_control(&c);
        assert_eq!(s, "RESUME \"a b.txt\" 0 77 t9");
        assert_eq!(parse_control(&s).unwrap(), c);
        let a = DccControl {
            kind: DccControlKind::Accept,
            token: None,
            port: 4000,
            ..c
        };
        assert_eq!(encode_control(&a), "ACCEPT \"a b.txt\" 4000 77");
        assert_eq!(parse_control(&encode_control(&a)).unwrap(), a);
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
