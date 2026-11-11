//! Spell checking for the composer. Windows uses the system spell checker
//! (the same one Edge and Office use, following the user's language settings);
//! Linux uses the system Hunspell dictionaries through `spellbook`.
//!
//! Offsets are UTF-16 code units, which is what QML string indices count.

/// A misspelled range: UTF-16 start and length.
pub type Range = (usize, usize);

#[cfg(windows)]
mod imp {
    use super::Range;
    use std::cell::RefCell;
    use windows::Win32::Globalization::{ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree,
    };
    use windows::core::{HSTRING, PWSTR};

    thread_local! {
        // COM objects stay on the thread that made them: the Qt GUI thread.
        static CHECKER: RefCell<Option<Option<ISpellChecker>>> = const { RefCell::new(None) };
    }

    fn with<R>(f: impl FnOnce(&ISpellChecker) -> R) -> Option<R> {
        CHECKER.with(|c| {
            let mut c = c.borrow_mut();
            if c.is_none() {
                *c = Some(create());
            }
            c.as_ref().unwrap().as_ref().map(f)
        })
    }

    fn create() -> Option<ISpellChecker> {
        unsafe {
            // Qt has usually initialised COM on this thread already; a mismatch error is fine.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let factory: ISpellCheckerFactory =
                CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let lang = user_language();
            for tag in [lang.as_str(), "en-US"] {
                let tag = HSTRING::from(tag);
                if factory
                    .IsSupported(&tag)
                    .map(|b| b.as_bool())
                    .unwrap_or(false)
                {
                    return factory.CreateSpellChecker(&tag).ok();
                }
            }
            None
        }
    }

    fn user_language() -> String {
        use windows::Win32::Globalization::GetUserDefaultLocaleName;
        let mut buf = [0u16; 85];
        let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
        if n > 1 {
            String::from_utf16_lossy(&buf[..n as usize - 1])
        } else {
            "en-US".into()
        }
    }

    unsafe fn take(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        let s = unsafe { p.to_string() }.unwrap_or_default();
        unsafe { CoTaskMemFree(Some(p.0 as _)) };
        s
    }

    pub fn check(text: &str) -> Vec<Range> {
        with(|sc| unsafe {
            let mut out = Vec::new();
            let Ok(errors) = sc.Check(&HSTRING::from(text)) else {
                return out;
            };
            loop {
                let mut e = None;
                if errors.Next(&mut e).is_err() {
                    break;
                }
                let Some(e) = e else { break };
                if let (Ok(s), Ok(l)) = (e.StartIndex(), e.Length()) {
                    out.push((s as usize, l as usize));
                }
            }
            out
        })
        .unwrap_or_default()
    }

    pub fn suggest(word: &str) -> Vec<String> {
        with(|sc| unsafe {
            let mut out = Vec::new();
            let Ok(list) = sc.Suggest(&HSTRING::from(word)) else {
                return out;
            };
            loop {
                let mut item = [PWSTR::null()];
                let mut got = 0u32;
                if list.Next(&mut item, Some(&mut got)).is_err() || got == 0 {
                    break;
                }
                out.push(take(item[0]));
                if out.len() >= 6 {
                    break;
                }
            }
            out
        })
        .unwrap_or_default()
    }

    pub fn add(word: &str) {
        with(|sc| unsafe {
            let _ = sc.Add(&HSTRING::from(word));
        });
    }

    /// Release the COM checker while COM is still up. Left to thread-local
    /// teardown, the release can run after COM shuts down and hang the exit.
    pub fn shutdown() {
        CHECKER.with(|c| c.borrow_mut().take());
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Range;
    use std::cell::RefCell;

    thread_local! {
        static DICT: RefCell<Option<Option<spellbook::Dictionary>>> = const { RefCell::new(None) };
        static ADDED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn load() -> Option<spellbook::Dictionary> {
        let lang = std::env::var("LANG")
            .ok()
            .and_then(|l| l.split('.').next().map(str::to_string))
            .unwrap_or_else(|| "en_US".into());
        for dir in [
            "/usr/share/hunspell",
            "/usr/share/myspell",
            "/usr/share/myspell/dicts",
        ] {
            for l in [lang.as_str(), "en_US"] {
                let aff = std::fs::read_to_string(format!("{dir}/{l}.aff"));
                let dic = std::fs::read_to_string(format!("{dir}/{l}.dic"));
                if let (Ok(a), Ok(d)) = (aff, dic)
                    && let Ok(dict) = spellbook::Dictionary::new(&a, &d)
                {
                    return Some(dict);
                }
            }
        }
        None
    }

    fn with<R>(f: impl FnOnce(&spellbook::Dictionary) -> R) -> Option<R> {
        DICT.with(|d| {
            let mut d = d.borrow_mut();
            if d.is_none() {
                *d = Some(load());
            }
            d.as_ref().unwrap().as_ref().map(f)
        })
    }

    pub fn check(text: &str) -> Vec<Range> {
        with(|dict| {
            let added = ADDED.with(|a| a.borrow().clone());
            super::words(text)
                .into_iter()
                .filter(|(w, _, _)| !dict.check(w) && !added.iter().any(|a| a == w))
                .map(|(_, s, l)| (s, l))
                .collect()
        })
        .unwrap_or_default()
    }

    pub fn suggest(word: &str) -> Vec<String> {
        with(|dict| {
            let mut out = Vec::new();
            dict.suggest(word, &mut out);
            out.truncate(6);
            out
        })
        .unwrap_or_default()
    }

    pub fn add(word: &str) {
        ADDED.with(|a| a.borrow_mut().push(word.to_string()));
    }

    pub fn shutdown() {}
}

pub use imp::{add, check, shutdown, suggest};

/// Words with their UTF-16 offsets: letters plus inner apostrophes.
#[cfg_attr(windows, allow(dead_code))]
pub fn words(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut start = 0;
    let mut pos = 0;
    for c in text.chars() {
        let w = c.len_utf16();
        if c.is_alphabetic() || (c == '\'' && !cur.is_empty()) {
            if cur.is_empty() {
                start = pos;
            }
            cur.push(c);
        } else if !cur.is_empty() {
            let t = cur.trim_end_matches('\'').to_string();
            let len = t.encode_utf16().count();
            out.push((t, start, len));
            cur.clear();
        }
        pos += w;
    }
    if !cur.is_empty() {
        let t = cur.trim_end_matches('\'').to_string();
        let len = t.encode_utf16().count();
        out.push((t, start, len));
    }
    out
}

/// Spans the checker should not judge: commands, URLs, channels, nicks, and
/// anything with digits or inner capitals (handles, code, acronyms).
pub fn ignorable(text: &str, start: usize, len: usize, nicks: &[String]) -> bool {
    let units: Vec<u16> = text.encode_utf16().collect();
    let Some(word) = units.get(start..start + len).map(String::from_utf16_lossy) else {
        return true;
    };
    // The whole whitespace-delimited token the word sits in.
    let mut a = start;
    while a > 0 && !char::from_u32(units[a - 1] as u32).is_some_and(char::is_whitespace) {
        a -= 1;
    }
    let mut b = start + len;
    while b < units.len() && !char::from_u32(units[b] as u32).is_some_and(char::is_whitespace) {
        b += 1;
    }
    let token = String::from_utf16_lossy(&units[a..b]);
    if a == 0 && token.starts_with('/') {
        return true;
    }
    if token.contains("://")
        || token.starts_with(['#', '@', ':'])
        || token.contains('.') && token.len() > 4
    {
        return true;
    }
    if word.chars().skip(1).any(char::is_uppercase) || token.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let lower = word.to_lowercase();
    nicks.iter().any(|n| n.to_lowercase() == lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_have_utf16_offsets() {
        let w = words("héllo 🎉 wörld don't");
        assert_eq!(w[0], ("héllo".into(), 0, 5));
        assert_eq!(w[1], ("wörld".into(), 9, 5));
        assert_eq!(w[2], ("don't".into(), 15, 5));
    }

    #[cfg(windows)]
    #[test]
    fn system_checker_finds_typos() {
        let r = check("this is teh best");
        assert_eq!(r, vec![(8, 3)]);
        assert!(suggest("teh").iter().any(|s| s == "the"));
    }

    #[test]
    fn ignores_irc_things() {
        let nicks = vec!["Alice".to_string()];
        let t = "/join #rustlang see https://exmaple.com alice iPhone x86 typo";
        let at = |w: &str| (t.find(w).unwrap(), w.len());
        for w in ["join", "rustlang", "exmaple", "alice", "iPhone", "x"] {
            let (s, l) = at(w);
            assert!(ignorable(t, s, l, &nicks), "{w} should be ignored");
        }
        let (s, l) = at("typo");
        assert!(!ignorable(t, s, l, &nicks));
    }
}
