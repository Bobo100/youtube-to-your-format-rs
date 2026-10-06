//! App logic with no Tauri dependency, so `cargo test` runs it without the
//! Windows app manifest that Tauri-linked test binaries need.

pub mod folders;
pub mod log;
pub mod media;
pub mod naming;
pub mod process;
pub mod queue;
pub mod runner;
pub mod tools;
pub mod ytdlp;

pub use tokio_util;

pub use reqwest;
