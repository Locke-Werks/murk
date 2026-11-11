//! Murk engine: connections, IRCv3 negotiation and tracked state, shared by every frontend.

pub mod config;
pub mod conn;
pub mod engine;
pub mod event;
pub mod input;
pub mod session;
pub mod state;
pub mod time;

pub use config::{NetworkConfig, Sasl};
pub use engine::Engine;
pub use event::{Command, ConnState, Event, Line, LineKind, NetworkId};
pub use murk_proto as proto;
pub use state::{Channel, Member, NetworkState};

#[cfg(test)]
mod tests;
