//! Murk IRC protocol layer. Pure data, no I/O.

pub mod casemap;
pub mod ctcp;
pub mod dcc;
pub mod format;
pub mod isupport;
pub mod message;

pub use message::{Message, ParseError, Prefix, Tag};
