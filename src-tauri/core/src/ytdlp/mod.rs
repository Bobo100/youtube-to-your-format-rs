//! Everything that talks to yt-dlp. Each call goes through `base_args` so the
//! JS runtime and UTF-8 output are never forgotten.

pub mod download;
pub mod errors;
pub mod input;
pub mod lookup;
pub mod progress;

use std::ffi::OsString;

use crate::tools::ToolPaths;

pub use input::Input;
pub use lookup::{Lookup, LookupKind, VideoCard};

/// Flags every yt-dlp invocation needs. YouTube downloads fail without a JS
/// runtime, and the official exe does not include one.
pub fn base_args(paths: &ToolPaths) -> Vec<OsString> {
    let mut deno = OsString::from("deno:");
    deno.push(paths.deno());
    vec![
        "--encoding".into(),
        "utf-8".into(),
        "--js-runtimes".into(),
        deno,
        "--ffmpeg-location".into(),
        paths.bin.clone().into_os_string(),
        "--no-color".into(),
    ]
}

pub(crate) fn is_network_failure(stderr: &str) -> bool {
    // "Unable to download …: HTTP Error 404" is a wrong link, not a dead network.
    if stderr.contains("HTTP Error 4") {
        return false;
    }
    [
        "getaddrinfo",
        "WinError 10060",
        "WinError 10061",
        "WinError 10065",
        "timed out",
        "Connection reset",
        "Connection refused",
        "Unable to download",
    ]
    .iter()
    .any(|needle| stderr.contains(needle))
}
