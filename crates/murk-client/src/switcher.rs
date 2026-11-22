//! Fuzzy buffer switching for frontends without a command palette of their own.

use crate::App;

/// An open switcher: what has been typed and which match is selected.
#[derive(Debug, Default, Clone)]
pub struct Switcher {
    pub query: String,
    pub sel: usize,
}

/// Scores `hay` against `needle` as a case-insensitive subsequence, or `None` if it is not one.
/// Consecutive runs and matches at word starts score higher, so "mur" ranks "#murk" above "#my-rust".
pub fn score(needle: &str, hay: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = hay.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0;
    let mut pos = 0;
    let mut prev: Option<usize> = None;
    for n in needle.chars().flat_map(char::to_lowercase) {
        let found = (pos..hay.len()).find(|&i| hay[i] == n)?;
        score += 1;
        if prev == Some(found.wrapping_sub(1)) {
            score += 4;
        }
        if found == 0 || !hay[found - 1].is_alphanumeric() {
            score += 3;
        }
        prev = Some(found);
        pos = found + 1;
    }
    // Shorter names win ties: "#rust" over "#rust-offtopic".
    Some(score * 100 - hay.len() as i32)
}

impl App {
    /// The label a switcher shows and matches against: "Network" or "Network/#chan".
    pub fn switcher_label(&self, idx: usize) -> String {
        let b = &self.buffers[idx];
        let net = self
            .networks
            .get(&b.network)
            .map(|n| n.name.as_str())
            .unwrap_or("?");
        match &b.name {
            Some(n) => format!("{net}/{n}"),
            None => net.to_string(),
        }
    }

    /// Buffers matching the query, best first. Matches on the buffer name rank above matches
    /// that only work through the network prefix.
    pub fn switcher_matches(&self, query: &str) -> Vec<usize> {
        let mut scored: Vec<(i32, usize)> = (0..self.buffers.len())
            .filter_map(|i| {
                let own = self.buffers[i].name.as_deref().and_then(|n| score(query, n));
                let full = score(query, &self.switcher_label(i));
                let s = match (own, full) {
                    (Some(a), _) => a + 50,
                    (None, Some(b)) => b,
                    (None, None) => return None,
                };
                // Unread buffers float up when the query does not separate them.
                let unread = self.buffers[i].highlighted as i32 * 20
                    + (self.buffers[i].unread > 0) as i32 * 10;
                Some((s + unread, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    pub fn open_switcher(&mut self) {
        self.switcher = Some(Switcher::default());
    }

    /// Switches to the selected match and closes the switcher.
    pub fn accept_switcher(&mut self) {
        if let Some(sw) = self.switcher.take()
            && let Some(&i) = self.switcher_matches(&sw.query).get(sw.sel)
        {
            self.switch(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn subsequence_required() {
        assert!(score("xyz", "#murk").is_none());
        assert!(score("mrk", "#murk").is_some());
    }

    #[test]
    fn runs_and_word_starts_rank_higher() {
        assert!(score("mur", "#murk") > score("mur", "#my-rust"));
        assert!(score("rust", "#rust") > score("rust", "#rust-offtopic"));
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(score("MURK", "#murk"), score("murk", "#murk"));
    }
}
