//! Single-line input editor with history and nick completion.

use unicode_width::UnicodeWidthStr;

const HISTORY_MAX: usize = 500;

#[derive(Default)]
pub struct Editor {
    text: String,
    /// Cursor position as a byte index on a char boundary.
    cursor: usize,
    history: Vec<String>,
    /// Index into history while browsing; `None` when editing a fresh line.
    hist_pos: Option<usize>,
    draft: String,
    completion: Option<Completion>,
}

struct Completion {
    start: usize,
    candidates: Vec<String>,
    index: usize,
    at_line_start: bool,
}

impl Editor {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Display column of the cursor.
    pub fn cursor_col(&self) -> usize {
        self.text[..self.cursor].width()
    }

    fn touched(&mut self) {
        self.completion = None;
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.touched();
    }

    pub fn insert_str(&mut self, s: &str) {
        let s: String = s
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
        self.touched();
    }

    fn prev_boundary(&self, i: usize) -> usize {
        self.text[..i]
            .char_indices()
            .next_back()
            .map(|(j, _)| j)
            .unwrap_or(0)
    }

    fn next_boundary(&self, i: usize) -> usize {
        self.text[i..]
            .chars()
            .next()
            .map(|c| i + c.len_utf8())
            .unwrap_or(i)
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let p = self.prev_boundary(self.cursor);
            self.text.replace_range(p..self.cursor, "");
            self.cursor = p;
        }
        self.touched();
    }

    pub fn delete(&mut self) {
        let n = self.next_boundary(self.cursor);
        self.text.replace_range(self.cursor..n, "");
        self.touched();
    }

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
        self.touched();
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
        self.touched();
    }

    pub fn home(&mut self) {
        self.cursor = 0;
        self.touched();
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
        self.touched();
    }

    pub fn kill_to_start(&mut self) {
        self.text.replace_range(..self.cursor, "");
        self.cursor = 0;
        self.touched();
    }

    pub fn kill_to_end(&mut self) {
        self.text.truncate(self.cursor);
        self.touched();
    }

    pub fn kill_word(&mut self) {
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end_matches(' ');
        let start = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.touched();
    }

    pub fn word_left(&mut self) {
        let before = self.text[..self.cursor].trim_end_matches(' ');
        self.cursor = before.rfind(' ').map(|i| i + 1).unwrap_or(0);
        self.touched();
    }

    pub fn word_right(&mut self) {
        let after = &self.text[self.cursor..];
        let skip = after.len() - after.trim_start_matches(' ').len();
        let rest = &after[skip..];
        self.cursor += skip + rest.find(' ').unwrap_or(rest.len());
        self.touched();
    }

    /// Take the line for submission and record it in history.
    pub fn take(&mut self) -> String {
        let line = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.hist_pos = None;
        self.touched();
        if !line.trim().is_empty() && self.history.last() != Some(&line) {
            self.history.push(line.clone());
            if self.history.len() > HISTORY_MAX {
                self.history.remove(0);
            }
        }
        line
    }

    fn set(&mut self, s: String) {
        self.cursor = s.len();
        self.text = s;
        self.touched();
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let pos = match self.hist_pos {
            None => {
                self.draft = self.text.clone();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.hist_pos = Some(pos);
        self.set(self.history[pos].clone());
    }

    pub fn history_down(&mut self) {
        match self.hist_pos {
            None => {}
            Some(p) if p + 1 >= self.history.len() => {
                self.hist_pos = None;
                let d = std::mem::take(&mut self.draft);
                self.set(d);
            }
            Some(p) => {
                self.hist_pos = Some(p + 1);
                self.set(self.history[p + 1].clone());
            }
        }
    }

    /// The word before the cursor when starting a new completion, `None` while cycling.
    pub fn completion_prefix(&self) -> Option<String> {
        if self.completion.is_some() {
            return None;
        }
        let start = self.text[..self.cursor]
            .rfind(' ')
            .map(|i| i + 1)
            .unwrap_or(0);
        Some(self.text[start..self.cursor].to_string())
    }

    /// Cycle completions for the word before the cursor. `words` supplies
    /// candidates, most relevant first; nicks completed at line start get ": ".
    pub fn complete(&mut self, words: impl FnOnce(&str) -> Vec<String>) {
        if let Some(c) = &mut self.completion {
            c.index = (c.index + 1) % c.candidates.len();
        } else {
            let start = self.text[..self.cursor]
                .rfind(' ')
                .map(|i| i + 1)
                .unwrap_or(0);
            let prefix = self.text[start..self.cursor].to_string();
            if prefix.is_empty() {
                return;
            }
            let candidates = words(&prefix);
            if candidates.is_empty() {
                return;
            }
            self.completion = Some(Completion {
                start,
                candidates,
                index: 0,
                at_line_start: start == 0,
            });
        }
        let c = self.completion.as_ref().expect("set above");
        let word = &c.candidates[c.index];
        let suffix = if c.at_line_start && !word.starts_with(['/', '#']) {
            ": "
        } else {
            " "
        };
        let replacement = format!("{word}{suffix}");
        let start = c.start;
        self.text.replace_range(start..self.cursor, &replacement);
        self.cursor = start + replacement.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Editor {
        let mut e = Editor::default();
        e.insert_str(s);
        e
    }

    #[test]
    fn editing_unicode() {
        let mut e = typed("héllo");
        e.left();
        e.left();
        e.backspace();
        assert_eq!(e.text(), "hélo");
        e.home();
        e.delete();
        assert_eq!(e.text(), "élo");
        e.end();
        e.kill_word();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn history_keeps_draft() {
        let mut e = typed("one");
        e.take();
        e.insert_str("two");
        e.take();
        e.insert_str("dra");
        e.history_up();
        assert_eq!(e.text(), "two");
        e.history_up();
        e.history_up();
        assert_eq!(e.text(), "one");
        e.history_down();
        e.history_down();
        assert_eq!(e.text(), "dra");
    }

    #[test]
    fn completion_cycles() {
        let mut e = typed("al");
        let cands = || vec!["alice".to_string(), "alan".to_string()];
        e.complete(|_| cands());
        assert_eq!(e.text(), "alice: ");
        e.complete(|_| cands());
        assert_eq!(e.text(), "alan: ");
        e.insert_str("hi ");
        e.insert_str("al");
        e.complete(|_| cands());
        assert_eq!(e.text(), "alan: hi alice ");
    }
}
