use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use super::progress::{LineEvent, ProgressTracker, PATH_PREFIX, PROGRESS_PREFIX};
use super::base_args;
use super::errors::classify;
use crate::tools::update::NeedsNewYtdlp;
use crate::naming::escape_template;
use crate::process::{self, SpawnError};
use crate::tools::ToolPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveFormat {
    #[default]
    Audio,
    Video,
}

impl SaveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Audio => "mp3",
            Self::Video => "mp4",
        }
    }

    fn streams(self) -> usize {
        match self {
            Self::Audio => 1,
            Self::Video => 2,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("tool: {0}")]
    Spawn(#[from] SpawnError),
    #[error("yt-dlp failed: {stderr}")]
    Failed { stderr: String },
    #[error("canceled")]
    Canceled,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl DownloadError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Spawn(SpawnError::Blocked(_)) => "tool_blocked",
            Self::Spawn(SpawnError::Missing(_)) => "tools_missing",
            Self::Spawn(SpawnError::TimedOut(_)) => "network",
            Self::Failed { stderr } => classify(stderr).code(),
            Self::Io(e) if e.kind() == std::io::ErrorKind::StorageFull => "disk_full",
            Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "folder_not_writable",
            Self::Canceled => "canceled",
            _ => "extractor",
        }
    }

    /// Last lines of yt-dlp's stderr, for "複製問題資訊".
    pub fn detail(&self) -> String {
        match self {
            Self::Failed { stderr } => tail(stderr, 30),
            other => other.to_string(),
        }
    }
}

impl NeedsNewYtdlp for DownloadError {
    fn needs_new_ytdlp(&self) -> bool {
        matches!(self, Self::Failed { stderr } if classify(stderr).may_be_fixed_by_update())
    }
}

pub(crate) fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// `cookies.txt` exported from a browser, for videos that need a login.
pub fn find_cookies() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("USERPROFILE")?);
    [home.join("cookies.txt"), home.join(".config").join("yt-cookies").join("cookies.txt")]
        .into_iter()
        .find(|p| p.is_file())
}

/// yt-dlp writes the cookie jar back to `--cookies FILE` when it exits (and a
/// kill mid-write truncates it), so each run gets a throwaway copy.
pub fn prepared_cookies() -> Option<tempfile::TempPath> {
    let source = find_cookies()?;
    let copy = tempfile::Builder::new().prefix("ytf-cookies-").suffix(".txt").tempfile().ok()?;
    std::fs::copy(&source, copy.path()).ok()?;
    Some(copy.into_temp_path())
}

pub fn download_args(
    paths: &ToolPaths,
    url: &str,
    format: SaveFormat,
    dir: &Path,
    base: &str,
    cookies: Option<&Path>,
) -> Vec<OsString> {
    let mut args = base_args(paths);
    // `<base>.ytf.<ext>` until the runner finishes the job (see `place_without_replacing`).
    let template = dir.join(format!("{}.{WORKING_MARK}.%(ext)s", escape_template(base)));
    args.extend(
        [
            "--newline",
            // --print implies --quiet; --progress keeps the progress lines coming.
            "--progress",
            "--progress-template",
            &format!("download:{PROGRESS_PREFIX}%(progress)j"),
            "--print",
            &format!("after_move:{PATH_PREFIX}%(filepath)s"),
            "--no-playlist",
            "--no-overwrites",
            "--embed-metadata",
        ]
        .map(OsString::from),
    );
    match format {
        SaveFormat::Audio => args.extend(
            ["-f", "ba/b", "-x", "--audio-format", "mp3", "--audio-quality", "0"].map(OsString::from),
        ),
        SaveFormat::Video => args.extend(
            [
                "-f",
                // H.264 + AAC merges into an mp4 every Windows player and LINE can open.
                "bv*[vcodec^=avc1][height<=1080]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b",
                "--merge-output-format",
                "mp4",
            ]
            .map(OsString::from),
        ),
    }
    if let Some(cookies) = cookies {
        args.extend([OsString::from("--cookies"), cookies.as_os_str().to_owned()]);
    }
    args.extend([OsString::from("-o"), template.into_os_string(), "--".into(), url.into()]);
    args
}

/// Marks files a job is still working on; still covered by the `<base>.` cleanup prefix.
pub const WORKING_MARK: &str = "ytf";

pub enum Update {
    Progress(f64),
    Processing,
}

/// Runs yt-dlp for one item. Returns the file it produced.
pub async fn run_download(
    paths: &ToolPaths,
    url: &str,
    format: SaveFormat,
    dir: &Path,
    base: &str,
    cancel: &CancellationToken,
    on_update: &(dyn Fn(Update) + Send + Sync),
) -> Result<PathBuf, DownloadError> {
    let cookies = prepared_cookies();
    let mut cmd = process::command(paths.ytdlp());
    cmd.args(download_args(paths, url, format, dir, base, cookies.as_deref()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut spawned = process::spawn(&mut cmd)?;
    let stdout = spawned.child.stdout.take().expect("stdout is piped");
    let mut stderr = spawned.child.stderr.take().expect("stderr is piped");
    let stderr_task = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        text
    });

    let mut tracker = ProgressTracker::new(format.streams());
    let mut final_path = None;
    let mut lines = BufReader::new(stdout).lines();
    let status = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                spawned.kill_tree();
                let _ = spawned.child.wait().await;
                return Err(DownloadError::Canceled);
            }
            line = lines.next_line() => match line? {
                Some(line) => match tracker.on_line(&line) {
                    Some(LineEvent::Progress(p)) => on_update(Update::Progress(p)),
                    Some(LineEvent::Processing) => on_update(Update::Processing),
                    Some(LineEvent::FinalPath(p)) => final_path = Some(PathBuf::from(p)),
                    None => {}
                },
                None => break spawned.child.wait().await?,
            },
        }
    };
    let stderr = stderr_task.await.unwrap_or_default();
    match final_path {
        Some(path) if status.success() && path.is_file() => Ok(path),
        _ => Err(DownloadError::Failed { stderr }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    fn args(format: SaveFormat, cookies: Option<&Path>) -> Vec<String> {
        let paths = ToolPaths { bin: PathBuf::from(r"C:\bin") };
        strings(download_args(
            &paths,
            "https://youtu.be/abc",
            format,
            Path::new(r"C:\Users\a\Downloads\YouTube"),
            "100% 好聽",
            cookies,
        ))
    }

    #[test]
    fn audio_extracts_mp3_into_an_escaped_template() {
        let a = args(SaveFormat::Audio, None);
        assert!(a.windows(2).any(|w| w == ["--audio-format", "mp3"]));
        assert!(a.windows(2).any(|w| w == ["--js-runtimes", r"deno:C:\bin\deno.exe"]));
        assert!(a.contains(&"--no-overwrites".to_owned()) && a.contains(&"--no-playlist".to_owned()));
        assert_eq!(
            &a[a.len() - 4..],
            ["-o", r"C:\Users\a\Downloads\YouTube\100%% 好聽.ytf.%(ext)s", "--", "https://youtu.be/abc"]
        );
        assert!(!a.contains(&"--cookies".to_owned()));
    }

    #[test]
    fn video_prefers_h264_and_falls_back() {
        let a = args(SaveFormat::Video, Some(Path::new(r"C:\Users\a\cookies.txt")));
        let selector = &a[a.iter().position(|x| x == "-f").unwrap() + 1];
        assert!(selector.starts_with("bv*[vcodec^=avc1]") && selector.ends_with("/b"));
        assert!(a.windows(2).any(|w| w == ["--merge-output-format", "mp4"]));
        assert!(a.windows(2).any(|w| w == ["--cookies", r"C:\Users\a\cookies.txt"]));
    }

    #[test]
    fn format_and_network_failures_get_their_own_codes() {
        let fail = |s: &str| DownloadError::Failed { stderr: s.into() }.code();
        assert_eq!(fail("ERROR: [youtube] x: Requested format is not available"), "format_unavailable");
        assert_eq!(fail("ERROR: Unable to download webpage: getaddrinfo failed"), "network");
        assert_eq!(fail("ERROR: [youtube] x: Video unavailable"), "unavailable");
        assert_eq!(fail("ERROR: [youtube] x: nsig extraction failed"), "extractor");
        let broken = DownloadError::Failed { stderr: "ERROR: [youtube] x: nsig extraction failed".into() };
        assert!(broken.needs_new_ytdlp());
        assert!(!DownloadError::Failed { stderr: "ERROR: [youtube] x: Private video".into() }.needs_new_ytdlp());
    }
}
