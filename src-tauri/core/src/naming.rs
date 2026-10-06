//! Output file names. A job owns every file in its folder that starts with
//! `<base>.` — `unique_base` guarantees no such file existed before — so
//! cancelling can safely delete them all.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// UTF-16 units, the way Windows counts path length: leaves room under the
/// 260-character MAX_PATH for the folder and `.f299.mp4.part` style suffixes.
const MAX_UNITS: usize = 100;

/// Turns a video title into a safe Windows file name stem.
pub fn sanitize(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut units = 0;
    let trimmed: String = collapsed
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= MAX_UNITS
        })
        .collect();
    let trimmed = trimmed.trim_end_matches(['.', ' ']).trim_start().to_owned();
    let reserved = {
        let upper = trimmed.split('.').next().unwrap_or_default().to_ascii_uppercase();
        matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (upper.len() == 4
                && (upper.starts_with("COM") || upper.starts_with("LPT"))
                && upper.as_bytes()[3].is_ascii_digit())
    };
    match (trimmed.is_empty(), reserved) {
        (true, _) => "youtube".to_owned(),
        (_, true) => format!("{trimmed}_"),
        _ => trimmed,
    }
}

/// `base`, `base (2)`, `base (3)`… — the first one no existing file starts with.
pub fn unique_base(dir: &Path, base: &str) -> String {
    let taken: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let free = |candidate: &str| {
        let prefix = format!("{}.", candidate.to_lowercase());
        !taken.iter().any(|name| name.starts_with(&prefix))
    };
    (1..)
        .map(|n| if n == 1 { base.to_owned() } else { format!("{base} ({n})") })
        .find(|candidate| free(candidate))
        .expect("an unbounded range always finds a free name")
}

/// Gives a finished working file its final name `<wanted>.<ext>` (or
/// `<wanted> (2).<ext>`…) without ever replacing an existing file. Downloads
/// and conversions both write under a working name first, so a process killed
/// mid-write (app closed, Windows shutting down) never leaves a truncated file
/// with a clean-looking name.
pub fn place_without_replacing(part: &Path, dir: &Path, wanted: &str, ext: &str) -> io::Result<PathBuf> {
    for n in 1.. {
        let name = if n == 1 { wanted.to_owned() } else { format!("{wanted} ({n})") };
        let target = dir.join(format!("{name}.{ext}"));
        match std::fs::hard_link(part, &target) {
            Ok(()) => {
                let _ = std::fs::remove_file(part);
                return Ok(target);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            // FAT32 USB sticks have no hard links; fall back to a checked rename.
            Err(_) if !target.exists() => {
                std::fs::rename(part, &target)?;
                return Ok(target);
            }
            Err(_) => continue,
        }
    }
    unreachable!("an unbounded range always finds a free name")
}

/// yt-dlp reads `-o` as a template: a literal `%` in a title must be doubled.
pub fn escape_template(text: &str) -> String {
    text.replace('%', "%%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_characters_windows_rejects() {
        assert_eq!(sanitize(r#"A/B: "C" <D>|E?*"#), "A_B_ _C_ _D__E__");
        assert_eq!(sanitize("  月亮代表我的心 . "), "月亮代表我的心");
        assert_eq!(sanitize("line\nbreak\ttab"), "line break tab");
    }

    #[test]
    fn handles_empty_reserved_and_long_titles() {
        assert_eq!(sanitize("???").trim_matches('_'), "");
        assert_eq!(sanitize("   "), "youtube");
        assert_eq!(sanitize("con"), "con_");
        assert_eq!(sanitize("COM1.mp3"), "COM1.mp3_");
        assert_eq!(sanitize(&"字".repeat(300)).chars().count(), MAX_UNITS);
        // Emoji are two UTF-16 units each.
        assert_eq!(sanitize(&"🎵".repeat(300)).encode_utf16().count(), MAX_UNITS);
    }

    #[test]
    fn unique_base_skips_any_file_sharing_the_prefix() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unique_base(dir.path(), "歌"), "歌");
        std::fs::write(dir.path().join("歌.mp3"), b"").unwrap();
        std::fs::write(dir.path().join("歌 (2).f140.m4a.part"), b"").unwrap();
        assert_eq!(unique_base(dir.path(), "歌"), "歌 (3)");
        // A different song merely starting with the same characters does not count.
        std::fs::write(dir.path().join("歌手.mp3"), b"").unwrap();
        assert_eq!(unique_base(dir.path(), "歌手 精選"), "歌手 精選");
    }

    #[test]
    fn unique_base_is_case_insensitive_like_ntfs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Song.mp3"), b"").unwrap();
        assert_eq!(unique_base(dir.path(), "song"), "song (2)");
    }

    #[test]
    fn escapes_percent_for_the_output_template() {
        assert_eq!(escape_template("100% 好聽"), "100%% 好聽");
    }
}
