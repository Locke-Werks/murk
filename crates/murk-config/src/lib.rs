//! User configuration: TOML on disk, secrets in the OS keyring.

use murk_core::{NetworkConfig, Sasl};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "murk";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("serializing config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("keyring: {0}")]
    Keyring(#[from] keyring::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A secret is either inline (discouraged) or looked up in the OS keyring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Secret {
    Keyring { keyring: String },
    Inline(String),
}

impl Secret {
    pub fn resolve(&self) -> Result<String> {
        match self {
            Secret::Inline(s) => Ok(s.clone()),
            Secret::Keyring { keyring } => {
                Ok(keyring::Entry::new(KEYRING_SERVICE, keyring)?.get_password()?)
            }
        }
    }

    /// Store `value` in the keyring under `key` and return a reference to it.
    pub fn store(key: &str, value: &str) -> Result<Secret> {
        keyring::Entry::new(KEYRING_SERVICE, key)?.set_password(value)?;
        Ok(Secret::Keyring {
            keyring: key.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "mechanism")]
pub enum SaslConfig {
    Plain { username: String, password: Secret },
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Network {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub tls_insecure: bool,
    pub nick: Option<String>,
    pub alt_nicks: Vec<String>,
    pub username: Option<String>,
    pub realname: Option<String>,
    pub password: Option<Secret>,
    pub sasl: Option<SaslConfig>,
    pub autojoin: Vec<String>,
    pub perform: Vec<String>,
    pub autoconnect: bool,
}

impl Default for Network {
    fn default() -> Self {
        Network {
            name: String::new(),
            host: String::new(),
            port: 6697,
            tls: true,
            tls_insecure: false,
            nick: None,
            alt_nicks: Vec::new(),
            username: None,
            realname: None,
            password: None,
            sasl: None,
            autojoin: Vec::new(),
            perform: Vec::new(),
            autoconnect: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub nick: String,
    pub username: Option<String>,
    pub realname: String,
    pub quit_message: String,
}

impl Default for Identity {
    fn default() -> Self {
        let user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "murk".into());
        let nick: String = user
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect();
        Identity {
            nick: if nick.is_empty() { "murk".into() } else { nick },
            username: None,
            realname: "Murk user".into(),
            quit_message: "Murk".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ui {
    pub theme: String,
    /// strftime-like: %H %M %S are supported.
    pub timestamp_format: String,
    pub show_joins_parts: bool,
    pub nicklist: bool,
    pub highlights: Vec<String>,
    pub scrollback_page: u32,
    /// Desktop layout: "cozy" groups messages under avatars, "compact" is one line per message.
    pub layout: String,
    pub font_size: u32,
    /// Show linked images and video thumbnails inline. Fetching them reveals your address to the host.
    pub inline_media: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            theme: "murk-dark".into(),
            timestamp_format: "%H:%M".into(),
            show_joins_parts: true,
            nicklist: true,
            highlights: Vec::new(),
            scrollback_page: 200,
            layout: "cozy".into(),
            font_size: 11,
            inline_media: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub identity: Identity,
    pub ui: Ui,
    #[serde(rename = "network")]
    pub networks: Vec<Network>,
    /// Raw `/alias name = expansion` entries; `$1`, `$2-`, `$*` expand arguments.
    pub aliases: std::collections::BTreeMap<String, String>,
    pub ignore: Vec<String>,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("murk")
}

/// Where logs live. `MURK_DATA_DIR` overrides it, for test runs and portable installs.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MURK_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("murk")
}

pub fn default_path() -> PathBuf {
    config_dir().join("murk.toml")
}

impl Config {
    /// Load from `path`, returning defaults with a Libera entry if it does not exist.
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).map_err(|source| Error::Parse {
                path: path.into(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::starter()),
            Err(source) => Err(Error::Io {
                path: path.into(),
                source,
            }),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let io = |source| Error::Io {
            path: path.into(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let text = toml::to_string_pretty(self)?;
        // Write then rename so a crash never leaves a half-written config.
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }

    pub fn starter() -> Config {
        Config {
            networks: vec![Network {
                name: "Libera".into(),
                host: "irc.libera.chat".into(),
                autoconnect: false,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    pub fn network(&self, name: &str) -> Option<&Network> {
        self.networks
            .iter()
            .find(|n| n.name.eq_ignore_ascii_case(name))
    }

    /// Build the engine config for a network, resolving secrets.
    pub fn engine_config(&self, net: &Network) -> Result<NetworkConfig> {
        let nick = net
            .nick
            .clone()
            .unwrap_or_else(|| self.identity.nick.clone());
        let mut c = NetworkConfig::new(&net.name, &net.host, &nick);
        c.port = net.port;
        c.tls = net.tls;
        c.tls_insecure = net.tls_insecure;
        c.alt_nicks = net.alt_nicks.clone();
        c.username = net
            .username
            .clone()
            .or_else(|| self.identity.username.clone())
            .unwrap_or(nick);
        c.realname = net
            .realname
            .clone()
            .unwrap_or_else(|| self.identity.realname.clone());
        c.password = net.password.as_ref().map(Secret::resolve).transpose()?;
        c.sasl = match &net.sasl {
            Some(SaslConfig::Plain { username, password }) => Some(Sasl::Plain {
                username: username.clone(),
                password: password.resolve()?,
            }),
            Some(SaslConfig::External) => Some(Sasl::External),
            None => None,
        };
        c.autojoin = net.autojoin.clone();
        c.perform = net.perform.clone();
        Ok(c)
    }
}

/// Expand an alias body with `$1`..`$9`, `$N-` (from N onward) and `$*`.
pub fn expand_alias(body: &str, args: &str) -> String {
    let words: Vec<&str> = args.split_whitespace().collect();
    let mut out = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('*') => {
                chars.next();
                out.push_str(args.trim());
            }
            Some(d @ '1'..='9') => {
                chars.next();
                let i = d as usize - '1' as usize;
                if chars.peek() == Some(&'-') {
                    chars.next();
                    out.push_str(&words.get(i..).unwrap_or(&[]).join(" "));
                } else {
                    out.push_str(words.get(i).copied().unwrap_or(""));
                }
            }
            _ => out.push('$'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_roundtrips() {
        let text = r##"
[identity]
nick = "archon"

[ui]
highlights = ["murk"]

[[network]]
name = "Libera"
host = "irc.libera.chat"
autojoin = ["#murk"]
sasl = { mechanism = "plain", username = "archon", password = { keyring = "libera" } }

[[network]]
name = "Local"
host = "127.0.0.1"
port = 6667
tls = false
password = "hunter2"

[aliases]
j = "/join $1"
"##;
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.identity.nick, "archon");
        assert_eq!(c.networks.len(), 2);
        assert_eq!(c.network("libera").unwrap().port, 6697);
        assert!(matches!(
            &c.networks[0].sasl,
            Some(SaslConfig::Plain { password: Secret::Keyring { keyring }, .. }) if keyring == "libera"
        ));
        let local = c.engine_config(c.network("Local").unwrap()).unwrap();
        assert_eq!(local.password.as_deref(), Some("hunter2"));
        assert_eq!(local.nick, "archon");

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("murk.toml");
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).unwrap(), c);
    }

    #[test]
    fn missing_file_gives_starter() {
        let c = Config::load(Path::new("definitely/not/here.toml")).unwrap();
        assert_eq!(c.networks[0].host, "irc.libera.chat");
    }

    #[test]
    fn aliases() {
        assert_eq!(
            expand_alias("/msg $1 hi $2-", "bob a b c"),
            "/msg bob hi a b c"
        );
        assert_eq!(expand_alias("say $* $$", " x y "), "say x y $$");
        assert_eq!(expand_alias("$3", "a"), "");
    }
}
