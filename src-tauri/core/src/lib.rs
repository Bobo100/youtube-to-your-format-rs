//! App logic with no Tauri dependency, so `cargo test` runs it without the
//! Windows app manifest that Tauri-linked test binaries need.

pub mod process;
pub mod tools;

pub use reqwest;
