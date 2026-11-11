//! Nick and channel case folding per the server's CASEMAPPING.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseMapping {
    Ascii,
    /// RFC 1459: also folds `[]\~` to `{}|^`. The historical default.
    #[default]
    Rfc1459,
    Rfc1459Strict,
}

impl CaseMapping {
    pub fn from_token(s: &str) -> CaseMapping {
        match s.to_ascii_lowercase().as_str() {
            "ascii" => CaseMapping::Ascii,
            "rfc1459-strict" | "strict-rfc1459" => CaseMapping::Rfc1459Strict,
            _ => CaseMapping::Rfc1459,
        }
    }

    pub fn fold_char(self, c: char) -> char {
        match (self, c) {
            (_, 'A'..='Z') => c.to_ascii_lowercase(),
            (CaseMapping::Ascii, _) => c,
            (_, '[') => '{',
            (_, ']') => '}',
            (_, '\\') => '|',
            (CaseMapping::Rfc1459, '~') => '^',
            _ => c,
        }
    }

    pub fn fold(self, s: &str) -> String {
        s.chars().map(|c| self.fold_char(c)).collect()
    }

    pub fn eq(self, a: &str, b: &str) -> bool {
        a.len() == b.len()
            && a.chars()
                .zip(b.chars())
                .all(|(x, y)| self.fold_char(x) == self.fold_char(y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds() {
        assert!(CaseMapping::Rfc1459.eq("Nick[a]~", "nick{a}^"));
        assert!(!CaseMapping::Rfc1459Strict.eq("a~", "a^"));
        assert!(!CaseMapping::Ascii.eq("[", "{"));
        assert!(CaseMapping::Ascii.eq("ABC", "abc"));
    }
}
