//! CTCP framing inside PRIVMSG/NOTICE bodies.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctcp<'a> {
    pub command: String,
    pub args: &'a str,
}

const DELIM: char = '\x01';

/// Returns the CTCP payload if `text` is a CTCP message. The closing delimiter
/// is optional, as many clients omit it.
pub fn parse(text: &str) -> Option<Ctcp<'_>> {
    let inner = text.strip_prefix(DELIM)?;
    let inner = inner.strip_suffix(DELIM).unwrap_or(inner);
    let (cmd, args) = inner.split_once(' ').unwrap_or((inner, ""));
    if cmd.is_empty() {
        return None;
    }
    Some(Ctcp {
        command: cmd.to_ascii_uppercase(),
        args,
    })
}

pub fn encode(command: &str, args: &str) -> String {
    if args.is_empty() {
        format!("{DELIM}{command}{DELIM}")
    } else {
        format!("{DELIM}{command} {args}{DELIM}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action() {
        let c = parse("\x01ACTION waves\x01").unwrap();
        assert_eq!(c.command, "ACTION");
        assert_eq!(c.args, "waves");
        assert_eq!(encode("ACTION", "waves"), "\x01ACTION waves\x01");
    }

    #[test]
    fn unterminated_and_bare() {
        assert_eq!(parse("\x01version").unwrap().command, "VERSION");
        assert!(parse("plain").is_none());
        assert!(parse("\x01\x01").is_none());
    }
}
