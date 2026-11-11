//! Folding runs of joins, parts, quits and nick changes into one summary line.
//! Shared so both clients fold the same way.

use murk_core::{Line, LineKind};
use std::collections::VecDeque;

/// Joins, parts, quits and nick changes: the lines that get folded together.
pub fn is_membership(k: &LineKind) -> bool {
    matches!(
        k,
        LineKind::Join | LineKind::Part { .. } | LineKind::Quit { .. } | LineKind::Nick { .. }
    )
}

/// Membership changes plus the mode lines servers emit around them, which fold too.
pub fn is_foldable(l: &Line) -> bool {
    is_membership(&l.kind)
        || (matches!(l.kind, LineKind::Mode)
            && l.sender
                .as_deref()
                .is_some_and(|s| s.contains('.') && !s.contains('!')))
}

/// For `lines[i]`: whether a later line in its run summarises it (so it is hidden),
/// and the summary when it is the last line of a run of two or more.
pub fn fold_at(lines: &VecDeque<Line>, i: usize) -> (bool, Option<String>) {
    let Some(l) = lines.get(i) else {
        return (false, None);
    };
    if !is_foldable(l) {
        return (false, None);
    }
    if lines.get(i + 1).is_some_and(is_foldable) {
        return (true, None);
    }
    let mut start = i;
    while start > 0 && i - start < 500 && is_foldable(&lines[start - 1]) {
        start -= 1;
    }
    let run: Vec<&Line> = lines
        .range(start..=i)
        .filter(|l| is_membership(&l.kind))
        .collect();
    if start < i && !run.is_empty() {
        (false, Some(fold_summary(&run)))
    } else {
        (false, None)
    }
}

/// One sentence for a run of membership lines, per person, in order of first appearance.
pub fn fold_summary(run: &[&Line]) -> String {
    #[derive(Default)]
    struct Who {
        acts: Vec<char>,
        renamed: Option<String>,
    }
    let mut order: Vec<String> = Vec::new();
    let mut people: std::collections::HashMap<String, Who> = Default::default();
    for l in run {
        let nick = l.sender.clone().unwrap_or_default();
        let key = nick.to_lowercase();
        if !people.contains_key(&key) {
            order.push(nick.clone());
        }
        let w = people.entry(key).or_default();
        match &l.kind {
            LineKind::Join => w.acts.push('j'),
            LineKind::Part { .. } | LineKind::Quit { .. } => w.acts.push('l'),
            LineKind::Nick { new } => w.renamed = Some(new.clone()),
            _ => {}
        }
    }
    let describe = |w: &Who| -> String {
        let a = &w.acts;
        let mut s = match (a.first(), a.last(), a.len()) {
            (None, _, _) => String::new(),
            (_, _, 1) if a[0] == 'j' => "joined".into(),
            (_, _, 1) => "left".into(),
            (Some('l'), Some('j'), 2) => "reconnected".into(),
            (Some('l'), Some('j'), n) => format!("reconnected {} times", n / 2),
            (Some('j'), Some('l'), 2) => "joined and left".into(),
            (_, Some('j'), n) => format!("joined and left {} times, then came back", n / 2),
            (_, _, n) => format!("joined and left {} times", n.div_ceil(2)),
        };
        if let Some(new) = &w.renamed {
            if !s.is_empty() {
                s.push_str(", ");
            }
            s.push_str(&format!("now known as {new}"));
        }
        s
    };
    if order.len() > 6 {
        let (mut joined, mut left) = (0, 0);
        for w in people.values() {
            match w.acts.last() {
                Some('j') => joined += 1,
                Some('l') => left += 1,
                _ => {}
            }
        }
        return format!("{joined} joined, {left} left, {} changes in all", run.len());
    }
    order
        .iter()
        .map(|n| format!("{n} {}", describe(&people[&n.to_lowercase()])))
        .collect::<Vec<_>>()
        .join(", ")
}
