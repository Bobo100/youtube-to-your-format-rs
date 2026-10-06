//! Turns yt-dlp's stderr into a reason the UI can explain in plain words, and
//! decides whether a newer yt-dlp might fix it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YtError {
    Network,
    /// Deleted, private, region-blocked, not started yet.
    Unavailable,
    /// "Sign in to confirm you're not a bot" / HTTP 429: wait, do not touch cookies.
    BotCheck,
    /// Age-restricted or members-only.
    LoginRequired,
    FormatUnavailable,
    DiskFull,
    /// A wrong link: HTTP 404 from YouTube, a malformed id, a missing playlist.
    NotFound,
    /// Writing the file failed on this computer (a file in use, no permission).
    LocalIo,
    /// Anything else — usually YouTube changed something yt-dlp does not know yet.
    Extractor,
}

impl YtError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Unavailable => "unavailable",
            Self::BotCheck => "bot_check",
            Self::LoginRequired => "login_required",
            Self::FormatUnavailable => "format_unavailable",
            Self::DiskFull => "disk_full",
            Self::NotFound => "lookup_failed",
            Self::LocalIo => "local_io",
            Self::Extractor => "extractor",
        }
    }

    /// YouTube breakage shows up as extractor errors and, with SABR-only
    /// streams, as "Requested format is not available".
    pub fn may_be_fixed_by_update(self) -> bool {
        matches!(self, Self::Extractor | Self::FormatUnavailable)
    }
}

/// The last `ERROR:` line decides; earlier WARNING lines are only context.
pub fn classify(stderr: &str) -> YtError {
    let error = stderr
        .lines()
        .rev()
        .find(|line| line.starts_with("ERROR:"))
        .unwrap_or(stderr);
    let has = |needles: &[&str]| needles.iter().any(|n| error.contains(n));

    if has(&["No space left on device", "[Errno 28]", "WinError 112", "WinError 39"]) {
        YtError::DiskFull
    } else if has(&[
        "WinError 5]",
        "WinError 32]",
        "[Errno 13]",
        "unable to open for writing",
        "unable to rename file",
        "Postprocessing:",
    ]) {
        YtError::LocalIo
    } else if has(&[
        "not a bot",
        "HTTP Error 429",
        "Too Many Requests",
        // YouTube's session throttle, worded like a dead video (both apostrophes occur).
        "content isn't available",
        "content isn\u{2019}t available",
        "try again later",
    ]) {
        YtError::BotCheck
    } else if has(&["confirm your age", "age-restricted", "members-only", "Join this channel", "Premium members"]) {
        YtError::LoginRequired
    } else if has(&["Requested format is not available"]) {
        YtError::FormatUnavailable
    } else if has(&[
        "Private video",
        "video is unavailable",
        "Video unavailable",
        "has been removed",
        "not available in your country",
        "uploader has not made this video available",
        "account associated with this video has been terminated",
        "This live event will begin",
        "Premieres in",
    ]) {
        YtError::Unavailable
    } else if has(&[
        "HTTP Error 404",
        "Incomplete YouTube ID",
        "is not a valid URL",
        "Unsupported URL",
        "does not exist",
    ]) {
        YtError::NotFound
    } else if super::is_network_failure(error) {
        YtError::Network
    } else {
        YtError::Extractor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Lines below are real yt-dlp 2026.08.19 output unless marked otherwise.

    #[test]
    fn classifies_real_messages() {
        let cases = [
            ("ERROR: [youtube] Brpwrk8kHH4: Private video", YtError::Unavailable),
            ("ERROR: [youtube] aaaaaaaaaaa: This video is unavailable", YtError::Unavailable),
            // yt-dlp 2025.01.15 against today's YouTube: the classic "outdated" symptom.
            ("ERROR: [youtube] jNQXAC9IVRw: The page needs to be reloaded.", YtError::Extractor),
            (
                "ERROR: [youtube:tab] PLxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx: YouTube said: The playlist does not exist.",
                YtError::NotFound,
            ),
            ("ERROR: [youtube:tab] UCxxxxxxxxxxxxxxxxxxxxxx: YouTube said: This channel does not exist.", YtError::NotFound),
            // From yt-dlp issue reports:
            (
                "ERROR: [youtube] abc: Video unavailable. This content isn\u{2019}t available, try again later.",
                YtError::BotCheck,
            ),
            ("ERROR: unable to rename file: [WinError 32] The process cannot access the file", YtError::LocalIo),
            ("ERROR: Postprocessing: Conversion failed!", YtError::LocalIo),
            (
                "ERROR: [youtube] jNQXAC9IVRw: Requested format is not available. Use --list-formats for a list of available formats",
                YtError::FormatUnavailable,
            ),
            (
                "ERROR: [youtube] 07FYdnEawAQ: Sign in to confirm your age. Use --cookies-from-browser or --cookies for the authentication.",
                YtError::LoginRequired,
            ),
            (
                "ERROR: [youtube] jNQXAC9IVRw: Unable to download API page: ('Unable to connect to proxy', NewConnectionError(\"... [WinError 10061] 無法連線，因為目標電腦拒絕連線。\")); please report this issue on  https://github.com/yt-dlp/yt-dlp/issues?q= , filling out the appropriate issue template. Confirm you are on the latest version using  yt-dlp -U",
                YtError::Network,
            ),
            // From yt-dlp issue reports (not reproducible on demand):
            (
                "ERROR: [youtube] abc: Sign in to confirm you\u{2019}re not a bot. Use --cookies-from-browser or --cookies for the authentication.",
                YtError::BotCheck,
            ),
            ("ERROR: unable to download video data: HTTP Error 429: Too Many Requests", YtError::BotCheck),
            ("ERROR: [youtube] abc: nsig extraction failed: Some formats may be missing", YtError::Extractor),
            ("ERROR: Unable to write to file: [Errno 28] No space left on device", YtError::DiskFull),
            (
                "ERROR: [youtube:tab] @TEDxTalks: Unable to download API page: HTTP Error 404: Not Found",
                YtError::NotFound,
            ),
        ];
        for (stderr, expected) in cases {
            assert_eq!(classify(stderr), expected, "{stderr}");
        }
    }

    #[test]
    fn the_last_error_line_wins_over_warnings() {
        let stderr = "WARNING: [youtube] Unable to download webpage: timed out. Retrying (1/3)...\nERROR: [youtube] x: Private video";
        assert_eq!(classify(stderr), YtError::Unavailable);
    }

    #[test]
    fn only_breakage_triggers_an_update() {
        assert!(YtError::Extractor.may_be_fixed_by_update());
        assert!(YtError::FormatUnavailable.may_be_fixed_by_update());
        assert!(!YtError::BotCheck.may_be_fixed_by_update());
        assert!(!YtError::Network.may_be_fixed_by_update());
    }
}
