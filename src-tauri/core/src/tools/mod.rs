//! Installs and verifies yt-dlp, ffmpeg/ffprobe and Deno under
//! `%LOCALAPPDATA%\youtube-to-your-format\bin\` (user-writable, no admin).

mod checksum;
mod download;
mod install;
mod manifest;
mod release;
mod state;
pub mod update;
pub mod version;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::Client;
use serde::Serialize;

use crate::process::{self, SpawnError};
use download::with_retry;
use install::{extract_7z, extract_zip, extract_zip_tree, new_path, replace_with_new, rollback};
pub(crate) use install::retry_io;
pub use checksum::sha256_file;
use manifest::{Pinned, DENO, FFMPEG};
use state::ToolState;

/// A freshly downloaded exe may sit in an antivirus scan before it runs.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),
    #[error("http status {0}")]
    Http(u16),
    #[error("server ignored the resume offset")]
    ResumeMismatch,
    #[error("checksum mismatch: {file}")]
    Checksum { file: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("archive: {0}")]
    Archive(String),
    #[error("release: {0}")]
    Release(String),
    #[error("blocked: {0}")]
    Blocked(String),
    #[error("does not run: {0}")]
    Broken(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl ToolError {
    /// Worth another attempt: the network or the server may recover.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) | Self::ResumeMismatch | Self::Checksum { .. } => true,
            Self::Http(status) => *status >= 500,
            _ => false,
        }
    }

    /// Error code shown to the frontend (wording lives in the i18n table).
    pub fn code(&self) -> &'static str {
        // ERROR_HANDLE_DISK_FULL 39, ERROR_DISK_FULL 112
        const DISK_FULL: [i32; 2] = [39, 112];
        match self {
            // Unauthenticated GitHub API: 60 requests/hour per IP, shared behind CGNAT.
            Self::Http(403 | 429) => "github_busy",
            Self::Network(_) | Self::Http(_) | Self::ResumeMismatch => "network",
            Self::Blocked(_) => "tool_blocked",
            Self::Io(e)
                if e.kind() == std::io::ErrorKind::StorageFull
                    || e.raw_os_error().is_some_and(|c| DISK_FULL.contains(&c)) =>
            {
                "disk_full"
            }
            Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "tool_blocked",
            _ => "tools_missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Ytdlp,
    Ffmpeg,
    Deno,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsProgress {
    pub step: usize,
    pub steps: usize,
    pub tool: Tool,
    pub phase: &'static str,
    pub received: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ToolPaths {
    pub bin: PathBuf,
}

impl ToolPaths {
    pub fn from_env() -> std::io::Result<Self> {
        let base = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| std::io::Error::other("LOCALAPPDATA is not set"))?;
        Ok(Self {
            bin: PathBuf::from(base).join("youtube-to-your-format").join("bin"),
        })
    }

    /// The onedir build lives in its own folder: no per-run unpacking into
    /// %TEMP% (a killed onefile exe leaks a 24 MB `_MEI*` folder every time).
    pub fn ytdlp_dir(&self) -> PathBuf {
        self.bin.join("yt-dlp")
    }
    pub fn ytdlp(&self) -> PathBuf {
        self.ytdlp_dir().join("yt-dlp.exe")
    }
    pub fn ffmpeg(&self) -> PathBuf {
        self.bin.join("ffmpeg.exe")
    }
    pub fn ffprobe(&self) -> PathBuf {
        self.bin.join("ffprobe.exe")
    }
    pub fn deno(&self) -> PathBuf {
        self.bin.join("deno.exe")
    }
    fn state(&self) -> PathBuf {
        self.bin.join("state.json")
    }
}

pub fn http_client() -> reqwest::Result<Client> {
    Client::builder()
        // GitHub's API rejects requests without a User-Agent.
        .user_agent(concat!("youtube-to-your-format/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
}

fn needed(paths: &ToolPaths, state: &ToolState) -> Vec<Tool> {
    let pinned_ok = |installed: &Option<String>, pinned: &Pinned, files: &[PathBuf]| {
        installed.as_deref() == Some(pinned.version) && files.iter().all(|f| f.exists())
    };
    let mut tools = Vec::new();
    if !(state.ytdlp.is_some() && paths.ytdlp().exists()) {
        tools.push(Tool::Ytdlp);
    }
    if !pinned_ok(&state.ffmpeg, &FFMPEG, &[paths.ffmpeg(), paths.ffprobe()]) {
        tools.push(Tool::Ffmpeg);
    }
    if !pinned_ok(&state.deno, &DENO, &[paths.deno()]) {
        tools.push(Tool::Deno);
    }
    tools
}

/// True when `prepare` has something to install (so it needs the write gate).
pub fn needs_install(paths: &ToolPaths) -> bool {
    !needed(paths, &ToolState::load(&paths.state())).is_empty()
}

/// Makes sure all three tools are installed; only downloads what is missing.
/// The caller holds `Updater::files()` (one owner for `bin/` and state.json)
/// and, when `needs_install`, the write gate so no tool is running.
pub async fn prepare(
    paths: &ToolPaths,
    client: &Client,
    emit: &(dyn Fn(ToolsProgress) + Sync),
) -> Result<(), ToolError> {
    tokio::fs::create_dir_all(&paths.bin).await?;
    let mut state = ToolState::load(&paths.state());
    let todo = needed(paths, &state);
    let steps = todo.len();

    for (index, tool) in todo.into_iter().enumerate() {
        let report = |phase: &'static str, received: u64, total: Option<u64>| {
            emit(ToolsProgress { step: index + 1, steps, tool, phase, received, total })
        };
        match tool {
            Tool::Ytdlp => {
                let release = with_retry(|| ytdlp_release(client, manifest::YTDLP_STABLE_REPO)).await?;
                let version = stage_ytdlp(paths, client, &release, &report).await?;
                swap_ytdlp(paths).await?;
                state.ytdlp_checked_at = Some(update::unix_now());
                // Earlier builds installed the onefile exe directly in bin/.
                let _ = tokio::fs::remove_file(paths.bin.join("yt-dlp.exe")).await;
                state.ytdlp = Some(version);
            }
            Tool::Ffmpeg => {
                install_pinned(paths, client, &FFMPEG, "ffmpeg.7z", &["ffmpeg.exe", "ffprobe.exe"], &report)
                    .await?;
                verify_or_rollback(&[
                    (paths.ffmpeg(), "-version", paths.ffmpeg()),
                    (paths.ffprobe(), "-version", paths.ffprobe()),
                ])
                .await?;
                state.ffmpeg = Some(FFMPEG.version.to_owned());
            }
            Tool::Deno => {
                install_pinned(paths, client, &DENO, "deno.zip", &["deno.exe"], &report).await?;
                verify_or_rollback(&[(paths.deno(), "--version", paths.deno())]).await?;
                state.deno = Some(DENO.version.to_owned());
            }
        }
        state.save(&paths.state())?;
    }
    Ok(())
}

type Report<'a> = dyn Fn(&'static str, u64, Option<u64>) + Sync + 'a;

/// Forwards download progress at most every 512 KB so the UI is not flooded.
fn throttled<'a>(report: &'a Report<'a>, last: &'a AtomicU64) -> impl Fn(u64, Option<u64>) + Sync + 'a {
    move |received, total| {
        let previous = last.load(Ordering::Relaxed);
        if received < previous || received - previous >= 512 * 1024 || Some(received) == total {
            last.store(received, Ordering::Relaxed);
            report("download", received, total);
        }
    }
}

async fn replace_all(targets: Vec<PathBuf>) -> Result<(), ToolError> {
    tokio::task::spawn_blocking(move || targets.iter().try_for_each(|t| replace_with_new(t)))
        .await
        .map_err(|e| ToolError::Internal(e.to_string()))?
        .map_err(ToolError::from)
}

async fn ytdlp_release(client: &Client, repo: &str) -> Result<release::Release, ToolError> {
    release::latest(client, repo, manifest::YTDLP_ASSET, manifest::YTDLP_SUMS_ASSET).await
}

/// Downloads the onedir build, checks its hash and unpacks it to `yt-dlp.new\`.
/// Safe while yt-dlp runs: nothing in use is touched until `swap_ytdlp`.
async fn stage_ytdlp(
    paths: &ToolPaths,
    client: &Client,
    release: &release::Release,
    report: &Report<'_>,
) -> Result<String, ToolError> {
    let sha256 = with_retry(|| release::expected_sha256(client, release, manifest::YTDLP_ASSET)).await?;
    let archive = paths.bin.join(manifest::YTDLP_ASSET);
    let last = AtomicU64::new(0);
    download::download_verified(client, &release.exe_url, &archive, &sha256, &throttled(report, &last)).await?;
    report("extract", 0, None);
    let dir = paths.ytdlp_dir();
    let archive_for_task = archive.clone();
    let files = tokio::task::spawn_blocking(move || extract_zip_tree(&archive_for_task, &dir))
        .await
        .map_err(|e| ToolError::Internal(e.to_string()))??;
    if !new_path(&paths.ytdlp_dir()).join("yt-dlp.exe").is_file() {
        return Err(ToolError::Archive(format!("{}: no yt-dlp.exe among {files} files", manifest::YTDLP_ASSET)));
    }
    let _ = tokio::fs::remove_file(&archive).await;
    Ok(release.version.clone())
}

/// Puts the staged build in place and checks it runs, rolling back if not.
/// No yt-dlp may be running: hold the updater's write gate.
async fn swap_ytdlp(paths: &ToolPaths) -> Result<(), ToolError> {
    replace_all(vec![paths.ytdlp_dir()]).await?;
    verify_or_rollback(&[(paths.ytdlp(), "--version", paths.ytdlp_dir())]).await
}

async fn install_pinned(
    paths: &ToolPaths,
    client: &Client,
    pinned: &Pinned,
    archive_name: &str,
    entries: &'static [&'static str],
    report: &Report<'_>,
) -> Result<(), ToolError> {
    let archive = paths.bin.join(archive_name);
    let last = AtomicU64::new(0);
    download::download_verified(client, pinned.url, &archive, pinned.sha256, &throttled(report, &last)).await?;

    report("extract", 0, None);
    let bin = paths.bin.clone();
    let archive_for_task = archive.clone();
    let found = tokio::task::spawn_blocking(move || {
        if archive_for_task.extension().is_some_and(|e| e == "7z") {
            extract_7z(&archive_for_task, entries, &bin)
        } else {
            extract_zip(&archive_for_task, entries, &bin)
        }
    })
    .await
    .map_err(|e| ToolError::Internal(e.to_string()))??;
    if found != entries.len() {
        return Err(ToolError::Archive(format!("{archive_name}: found {found}/{}", entries.len())));
    }
    replace_all(entries.iter().map(|e| paths.bin.join(e)).collect()).await?;
    let _ = tokio::fs::remove_file(&archive).await;
    Ok(())
}

/// Runs each freshly installed exe; if any fails, every installed target (a
/// file, or yt-dlp's folder) goes back to its previous version.
async fn verify_or_rollback(checks: &[(PathBuf, &str, PathBuf)]) -> Result<(), ToolError> {
    for (exe, arg, _) in checks {
        if let Err(err) = verify_runs(exe, arg).await {
            let targets: Vec<PathBuf> = checks.iter().map(|(_, _, target)| target.clone()).collect();
            let _ = tokio::task::spawn_blocking(move || targets.iter().try_for_each(|t| rollback(t))).await;
            return Err(err);
        }
    }
    Ok(())
}

async fn verify_runs(exe: &Path, arg: &str) -> Result<(), ToolError> {
    let label = exe.display().to_string();
    match process::run(process::command(exe).arg(arg), VERIFY_TIMEOUT).await {
        Ok(out) if out.status.success() => Ok(()),
        Ok(_) | Err(SpawnError::TimedOut(_)) => Err(ToolError::Broken(label)),
        // The file was just written, so "missing" means it was quarantined.
        Err(SpawnError::Blocked(_) | SpawnError::Missing(_)) => Err(ToolError::Blocked(label)),
        Err(SpawnError::Internal(e)) => Err(ToolError::Internal(e.to_string())),
        Err(SpawnError::Other(e)) => Err(ToolError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn error_codes_tell_the_user_what_to_do() {
        use std::io::{Error, ErrorKind};
        assert_eq!(ToolError::Http(403).code(), "github_busy");
        assert_eq!(ToolError::Http(429).code(), "github_busy");
        assert_eq!(ToolError::Http(502).code(), "network");
        assert_eq!(ToolError::Io(Error::from_raw_os_error(112)).code(), "disk_full");
        assert_eq!(ToolError::Io(Error::from(ErrorKind::PermissionDenied)).code(), "tool_blocked");
        assert_eq!(ToolError::Internal("job".into()).code(), "tools_missing");
        assert!(ToolError::Http(503).is_transient() && !ToolError::Http(404).is_transient());
    }

    #[test]
    fn fresh_install_needs_everything() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ToolPaths { bin: dir.path().into() };
        assert_eq!(needed(&paths, &ToolState::default()), vec![Tool::Ytdlp, Tool::Ffmpeg, Tool::Deno]);
    }

    #[test]
    fn up_to_date_install_needs_nothing_and_pin_bump_reinstalls() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ToolPaths { bin: dir.path().into() };
        for f in [paths.ytdlp(), paths.ffmpeg(), paths.ffprobe(), paths.deno()] {
            touch(&f);
        }
        let mut state = ToolState {
            ytdlp: Some("2026.08.19".into()),
            ffmpeg: Some(FFMPEG.version.into()),
            deno: Some(DENO.version.into()),
            ytdlp_checked_at: None,
        };
        assert!(needed(&paths, &state).is_empty());

        state.deno = Some("2.0.0".into());
        assert_eq!(needed(&paths, &state), vec![Tool::Deno]);
    }

    #[test]
    fn missing_ffprobe_reinstalls_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ToolPaths { bin: dir.path().into() };
        for f in [paths.ytdlp(), paths.ffmpeg(), paths.deno()] {
            touch(&f);
        }
        let state = ToolState {
            ytdlp: Some("2026.08.19".into()),
            ffmpeg: Some(FFMPEG.version.into()),
            deno: Some(DENO.version.into()),
            ytdlp_checked_at: None,
        };
        assert_eq!(needed(&paths, &state), vec![Tool::Ffmpeg]);
    }

    /// Real downloads (~95 MB). Run with `npm run rs:test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn installs_real_tools_into_a_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ToolPaths { bin: dir.path().into() };
        prepare(&paths, &http_client().unwrap(), &|_| {}).await.unwrap();
        assert!(needed(&paths, &ToolState::load(&paths.state())).is_empty());
    }
}
