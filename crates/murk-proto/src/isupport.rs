//! RPL_ISUPPORT (005) token tracking.

use crate::casemap::CaseMapping;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ISupport {
    tokens: HashMap<String, Option<String>>,
    pub casemapping: CaseMapping,
    pub chantypes: String,
    /// Mode letters and their prefix symbols, highest rank first, e.g. `(ov)@+`.
    pub prefix_modes: Vec<(char, char)>,
    /// CHANMODES groups A (list), B (always param), C (param on set), D (no param).
    pub chanmodes: [String; 4],
    pub network: Option<String>,
}

impl Default for ISupport {
    fn default() -> Self {
        ISupport {
            tokens: HashMap::new(),
            casemapping: CaseMapping::Rfc1459,
            chantypes: "#&".into(),
            prefix_modes: vec![('o', '@'), ('v', '+')],
            chanmodes: ["beI".into(), "k".into(), "l".into(), "imnpst".into()],
            network: None,
        }
    }
}

impl ISupport {
    /// Apply the token params of a 005 line (excluding the target nick and trailing text).
    pub fn apply<'a>(&mut self, tokens: impl IntoIterator<Item = &'a str>) {
        for tok in tokens {
            if let Some(key) = tok.strip_prefix('-') {
                self.tokens.remove(&key.to_ascii_uppercase());
                continue;
            }
            let (k, v) = match tok.split_once('=') {
                Some((k, v)) => (k.to_ascii_uppercase(), Some(unescape(v))),
                None => (tok.to_ascii_uppercase(), None),
            };
            match (k.as_str(), v.as_deref()) {
                ("CASEMAPPING", Some(v)) => self.casemapping = CaseMapping::from_token(v),
                ("CHANTYPES", Some(v)) => self.chantypes = v.to_string(),
                ("NETWORK", Some(v)) => self.network = Some(v.to_string()),
                ("PREFIX", Some(v)) => {
                    if let Some(p) = parse_prefix(v) {
                        self.prefix_modes = p;
                    }
                }
                ("CHANMODES", Some(v)) => {
                    let mut it = v.splitn(4, ',');
                    for slot in self.chanmodes.iter_mut() {
                        *slot = it.next().unwrap_or("").to_string();
                    }
                }
                _ => {}
            }
            self.tokens.insert(k, v);
        }
    }

    pub fn get(&self, key: &str) -> Option<Option<&str>> {
        self.tokens
            .get(&key.to_ascii_uppercase())
            .map(|v| v.as_deref())
    }

    /// Whether the server relays a client-only tag (`+draft/react` is asked as
    /// `draft/react`), per CLIENTTAGDENY: `*` denies all, `-name` exempts one.
    pub fn client_tag_allowed(&self, tag: &str) -> bool {
        let tag = tag.trim_start_matches('+');
        let Some(Some(list)) = self.get("CLIENTTAGDENY") else {
            return true;
        };
        let mut allowed = true;
        for entry in list.split(',') {
            match entry {
                "*" => allowed = false,
                e if e.strip_prefix('-') == Some(tag) => return true,
                e if e == tag => return false,
                _ => {}
            }
        }
        allowed
    }

    pub fn is_channel(&self, target: &str) -> bool {
        target
            .chars()
            .next()
            .is_some_and(|c| self.chantypes.contains(c))
    }

    pub fn prefix_for_mode(&self, mode: char) -> Option<char> {
        self.prefix_modes
            .iter()
            .find(|(m, _)| *m == mode)
            .map(|(_, p)| *p)
    }

    pub fn mode_for_prefix(&self, prefix: char) -> Option<char> {
        self.prefix_modes
            .iter()
            .find(|(_, p)| *p == prefix)
            .map(|(m, _)| *m)
    }

    /// Rank of a prefix symbol; lower is more privileged.
    pub fn prefix_rank(&self, prefix: char) -> Option<usize> {
        self.prefix_modes.iter().position(|(_, p)| *p == prefix)
    }

    /// Whether `mode` takes a parameter when being set (`adding`) or unset.
    pub fn mode_takes_param(&self, mode: char, adding: bool) -> bool {
        if self.prefix_for_mode(mode).is_some() {
            return true;
        }
        let [a, b, c, _] = &self.chanmodes;
        a.contains(mode) || b.contains(mode) || (adding && c.contains(mode))
    }
}

fn parse_prefix(v: &str) -> Option<Vec<(char, char)>> {
    let rest = v.strip_prefix('(')?;
    let (modes, symbols) = rest.split_once(')')?;
    if modes.chars().count() != symbols.chars().count() {
        return None;
    }
    Some(modes.chars().zip(symbols.chars()).collect())
}

/// ISUPPORT values escape bytes as `\xHH`.
fn unescape(v: &str) -> String {
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1] == b'x' {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 2]), hex(bytes[i + 3])) {
                out.push((h * 16 + l) as u8);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_tag_deny() {
        let mut i = ISupport::default();
        assert!(i.client_tag_allowed("+draft/react"));
        i.apply(["CLIENTTAGDENY=*,-typing,-channel-context"]);
        assert!(!i.client_tag_allowed("+draft/react"));
        assert!(i.client_tag_allowed("+typing"));
        let mut j = ISupport::default();
        j.apply(["CLIENTTAGDENY=draft/react"]);
        assert!(!j.client_tag_allowed("draft/react"));
        assert!(j.client_tag_allowed("draft/reply"));
    }

    #[test]
    fn applies_tokens() {
        let mut s = ISupport::default();
        s.apply([
            "CASEMAPPING=ascii",
            "PREFIX=(qaohv)~&@%+",
            "CHANMODES=beI,k,l,imnpst",
            "NETWORK=Test\\x20Net",
            "CHANTYPES=#",
        ]);
        assert_eq!(s.casemapping, CaseMapping::Ascii);
        assert_eq!(s.prefix_for_mode('h'), Some('%'));
        assert_eq!(s.prefix_rank('~'), Some(0));
        assert_eq!(s.network.as_deref(), Some("Test Net"));
        assert!(s.is_channel("#a"));
        assert!(!s.is_channel("&a"));
        assert!(s.mode_takes_param('l', true));
        assert!(!s.mode_takes_param('l', false));
        assert!(s.mode_takes_param('o', false));
        s.apply(["-NETWORK"]);
        assert_eq!(s.get("NETWORK"), None);
    }
}
