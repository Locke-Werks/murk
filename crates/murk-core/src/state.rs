//! Tracked network state: channels, members, modes.

use murk_proto::isupport::ISupport;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Member {
    pub nick: String,
    /// Prefix symbols held, highest rank first (multi-prefix).
    pub prefixes: String,
    pub user: Option<String>,
    pub host: Option<String>,
    pub account: Option<String>,
    pub away: bool,
}

impl Member {
    pub fn highest_prefix(&self) -> Option<char> {
        self.prefixes.chars().next()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Channel {
    pub name: String,
    pub topic: Option<String>,
    pub topic_setter: Option<String>,
    /// Keyed by casefolded nick.
    pub members: BTreeMap<String, Member>,
    pub modes: String,
    /// True while a NAMES reply is streaming in, so it replaces rather than merges.
    pub(crate) names_pending: bool,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkState {
    pub nick: String,
    pub isupport: ISupport,
    pub caps: HashSet<String>,
    /// Keyed by casefolded channel name.
    pub channels: BTreeMap<String, Channel>,
    pub registered: bool,
    pub away: bool,
}

impl NetworkState {
    pub fn fold(&self, s: &str) -> String {
        self.isupport.casemapping.fold(s)
    }

    pub fn is_me(&self, nick: &str) -> bool {
        self.isupport.casemapping.eq(nick, &self.nick)
    }

    pub fn has_cap(&self, cap: &str) -> bool {
        self.caps.contains(cap)
    }

    pub fn channel(&self, name: &str) -> Option<&Channel> {
        self.channels.get(&self.fold(name))
    }

    pub(crate) fn channel_mut(&mut self, name: &str) -> Option<&mut Channel> {
        let key = self.fold(name);
        self.channels.get_mut(&key)
    }

    /// Sort members by rank then name, the order a nick list shows them.
    pub fn sorted_members(&self, channel: &str) -> Vec<Member> {
        let Some(ch) = self.channel(channel) else {
            return Vec::new();
        };
        let mut v: Vec<Member> = ch.members.values().cloned().collect();
        let rank = |m: &Member| {
            m.highest_prefix()
                .and_then(|p| self.isupport.prefix_rank(p))
                .unwrap_or(usize::MAX)
        };
        v.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| self.fold(&a.nick).cmp(&self.fold(&b.nick)))
        });
        v
    }

    /// Split a NAMES entry like `@+nick!user@host` into prefixes and member.
    pub(crate) fn parse_names_entry(&self, entry: &str) -> Member {
        let split = entry
            .find(|c: char| self.isupport.prefix_rank(c).is_none())
            .unwrap_or(entry.len());
        let (prefixes, rest) = entry.split_at(split);
        let p = murk_proto::Prefix::parse(rest);
        let (nick, user, host) = match p {
            murk_proto::Prefix::User { nick, user, host } => (nick, user, host),
            murk_proto::Prefix::Server(s) => (s, None, None),
        };
        Member {
            nick,
            prefixes: prefixes.to_string(),
            user,
            host,
            ..Default::default()
        }
    }

    /// Insert or remove a prefix symbol on a member, keeping rank order.
    pub(crate) fn set_prefix(&mut self, channel: &str, nick: &str, symbol: char, on: bool) {
        let key = self.fold(nick);
        let isupport = self.isupport.clone();
        if let Some(m) = self
            .channel_mut(channel)
            .and_then(|c| c.members.get_mut(&key))
        {
            let mut set: Vec<char> = m.prefixes.chars().filter(|c| *c != symbol).collect();
            if on {
                set.push(symbol);
            }
            set.sort_by_key(|c| isupport.prefix_rank(*c).unwrap_or(usize::MAX));
            m.prefixes = set.into_iter().collect();
        }
    }
}
