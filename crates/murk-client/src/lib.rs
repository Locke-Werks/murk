//! Client state shared by every Murk frontend: buffers, unread tracking, command
//! dispatch and the input editor. Frontends own only drawing and input mapping.

pub mod app;
pub mod editor;
pub mod emoji;

pub use app::{App, Buffer, Network};
pub use editor::Editor;
