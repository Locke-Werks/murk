//! Per-network connection settings.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sasl {
    Plain {
        username: String,
        password: String,
    },
    /// Authenticate with the TLS client certificate.
    External,
}

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// Accept invalid certificates. Only for self-signed test servers.
    pub tls_insecure: bool,
    pub nick: String,
    pub alt_nicks: Vec<String>,
    pub username: String,
    pub realname: String,
    pub password: Option<String>,
    pub sasl: Option<Sasl>,
    pub autojoin: Vec<String>,
    /// Raw lines sent after registration, before autojoin.
    pub perform: Vec<String>,
    pub reconnect: bool,
}

impl NetworkConfig {
    pub fn new(name: impl Into<String>, host: impl Into<String>, nick: impl Into<String>) -> Self {
        let nick = nick.into();
        NetworkConfig {
            name: name.into(),
            host: host.into(),
            port: 6697,
            tls: true,
            tls_insecure: false,
            username: nick.clone(),
            realname: nick.clone(),
            nick,
            alt_nicks: Vec::new(),
            password: None,
            sasl: None,
            autojoin: Vec::new(),
            perform: Vec::new(),
            reconnect: true,
        }
    }
}
