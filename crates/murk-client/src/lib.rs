//! Client state shared by every Murk frontend: buffers, unread tracking, command
//! dispatch and the input editor. Frontends own only drawing and input mapping.

pub mod app;
pub mod switcher;
pub mod dcc;
pub mod editor;
pub mod emoji;
pub mod fold;

pub use app::{App, Buffer, Network, NotifyLevel};
pub use editor::Editor;
