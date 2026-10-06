//! Pinned tool builds. Upgrading ffmpeg or Deno = edit these constants and ship
//! a new app release. Hashes were checked against the publisher's own checksum
//! (gyan.dev `.sha256`, Deno `.sha256sum`) when pinned.

pub struct Pinned {
    pub version: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
}

/// gyan.dev essentials build (includes libx264 + ffprobe). GyanD/codexffmpeg keeps
/// every versioned release (back to 2020), unlike BtbN which prunes old builds.
pub const FFMPEG: Pinned = Pinned {
    version: "9.0.2",
    url: "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.7z",
    sha256: "4705843ccaaf54257c16ad90f3e952ece33c17df964ecf7bfdbb0f49c7171077",
};

/// yt-dlp needs an external JS runtime for YouTube; Deno is its default (min 2.3.0).
pub const DENO: Pinned = Pinned {
    version: "2.9.7",
    url: "https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-pc-windows-msvc.zip",
    sha256: "a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238",
};

pub const YTDLP_STABLE_REPO: &str = "yt-dlp/yt-dlp";
pub const YTDLP_ASSET: &str = "yt-dlp.exe";
pub const YTDLP_SUMS_ASSET: &str = "SHA2-256SUMS";
