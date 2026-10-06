//! Local file conversion: anything ffmpeg can read → MP3 or MP4.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::naming::sanitize;
use crate::process::{self, SpawnError};
use crate::queue::SaveFormat;
use crate::tools::ToolPaths;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("tool: {0}")]
    Spawn(#[from] SpawnError),
    #[error("ffmpeg failed: {0}")]
    Failed(String),
    #[error("canceled")]
    Canceled,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl ConvertError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Spawn(SpawnError::Blocked(_)) => "tool_blocked",
            Self::Spawn(SpawnError::Missing(_)) => "tools_missing",
            Self::Io(e) if e.kind() == std::io::ErrorKind::StorageFull => "disk_full",
            Self::Io(e) if e.kind() == std::io::ErrorKind::NotFound => "source_missing",
            Self::Canceled => "canceled",
            _ => "convert_failed",
        }
    }
}

/// Where a conversion of `source` goes:
/// - next to the source, same name with the new extension: `歌.wav` → `歌.mp3`
/// - already in the target format: `歌 (轉檔).mp4`
/// - a name collision adds ` (2)`
///
/// `fallback_dir` is used when the source folder cannot be written (a CD, a
/// read-only share).
///
/// Unlike downloads this only checks the exact output name, never a shared
/// prefix: the source itself shares the prefix (`歌.wav` / `歌.mp3`), so a
/// conversion must only ever delete the single file it wrote.
pub fn output_path(source: &Path, format: SaveFormat, fallback_dir: &Path) -> PathBuf {
    let stem = sanitize(&source.file_stem().unwrap_or_default().to_string_lossy());
    let same_format = source
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension()));
    let wanted = if same_format { format!("{stem} (轉檔)") } else { stem };
    let dir = source
        .parent()
        .filter(|dir| is_writable(dir))
        .map_or_else(|| fallback_dir.to_path_buf(), Path::to_path_buf);
    let ext = format.extension();
    (1..)
        .map(|n| if n == 1 { wanted.clone() } else { format!("{wanted} ({n})") })
        .map(|name| dir.join(format!("{name}.{ext}")))
        .find(|candidate| !candidate.exists() && candidate != source)
        .expect("an unbounded range always finds a free name")
}

fn is_writable(dir: &Path) -> bool {
    tempfile::Builder::new().prefix(".ytf-write-test").tempfile_in(dir).is_ok()
}

pub fn ffmpeg_args(source: &Path, output: &Path, format: SaveFormat) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = ["-nostdin", "-hide_banner", "-v", "error", "-nostats", "-progress", "pipe:1", "-n", "-i"]
        .map(Into::into)
        .to_vec();
    args.push(source.as_os_str().to_owned());
    let codec: &[&str] = match format {
        SaveFormat::Audio => &["-vn", "-c:a", "libmp3lame", "-q:a", "0"],
        SaveFormat::Video => &[
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a",
            "192k", "-movflags", "+faststart",
        ],
    };
    args.extend(codec.iter().map(Into::into));
    args.push(output.as_os_str().to_owned());
    args
}

/// Reads `out_time_us=…` from ffmpeg's `-progress` output as a 0–1 fraction.
pub fn parse_progress(line: &str, total_us: Option<u64>) -> Option<f64> {
    let value = line.strip_prefix("out_time_us=")?.trim().parse::<u64>().ok()?;
    let total = total_us.filter(|t| *t > 0)?;
    Some((value as f64 / total as f64).clamp(0.0, 0.99))
}

pub async fn duration_us(paths: &ToolPaths, source: &Path) -> Option<u64> {
    let out = process::run(
        process::command(paths.ffprobe())
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
            .arg(source),
        PROBE_TIMEOUT,
    )
    .await
    .ok()?;
    let seconds: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some((seconds * 1_000_000.0) as u64)
}

pub async fn run_convert(
    paths: &ToolPaths,
    source: &Path,
    output: &Path,
    format: SaveFormat,
    cancel: &CancellationToken,
    on_progress: &(dyn Fn(Option<f64>) + Send + Sync),
) -> Result<PathBuf, ConvertError> {
    if !source.is_file() {
        return Err(ConvertError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)));
    }
    let total = duration_us(paths, source).await;
    let mut cmd = process::command(paths.ffmpeg());
    cmd.args(ffmpeg_args(source, output, format)).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut spawned = process::spawn(&mut cmd)?;
    let stdout = spawned.child.stdout.take().expect("stdout is piped");
    let mut stderr = spawned.child.stderr.take().expect("stderr is piped");
    let stderr_task = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        text
    });
    let mut lines = BufReader::new(stdout).lines();
    let status = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                spawned.kill_tree();
                let _ = spawned.child.wait().await;
                let _ = tokio::fs::remove_file(output).await;
                return Err(ConvertError::Canceled);
            }
            line = lines.next_line() => match line? {
                Some(line) => {
                    if let Some(p) = parse_progress(&line, total) {
                        on_progress(Some(p));
                    } else if total.is_none() && line.starts_with("out_time_us=") {
                        on_progress(None);
                    }
                }
                None => break spawned.child.wait().await?,
            },
        }
    };
    if !status.success() {
        let _ = tokio::fs::remove_file(output).await;
        return Err(ConvertError::Failed(stderr_task.await.unwrap_or_default()));
    }
    Ok(output.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_goes_next_to_the_source_with_the_new_extension() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let source = dir.path().join("歌.wav");
        std::fs::write(&source, b"").unwrap();
        assert_eq!(output_path(&source, SaveFormat::Audio, fallback.path()), dir.path().join("歌.mp3"));
    }

    #[test]
    fn same_format_gets_a_marked_name_and_collisions_are_numbered() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let source = dir.path().join("影片.MP4");
        std::fs::write(&source, b"").unwrap();
        let first = output_path(&source, SaveFormat::Video, fallback.path());
        assert_eq!(first, dir.path().join("影片 (轉檔).mp4"));
        std::fs::write(&first, b"").unwrap();
        assert_eq!(output_path(&source, SaveFormat::Video, fallback.path()), dir.path().join("影片 (轉檔) (2).mp4"));
        // The source shares the `影片.` prefix but is not the output name.
        assert_eq!(output_path(&source, SaveFormat::Audio, fallback.path()), dir.path().join("影片.mp3"));
    }

    #[test]
    fn unwritable_folder_falls_back() {
        let fallback = tempfile::tempdir().unwrap();
        let source = Path::new(r"Z:\no-such-drive\歌.wav");
        assert_eq!(output_path(source, SaveFormat::Audio, fallback.path()), fallback.path().join("歌.mp3"));
    }

    #[test]
    fn reads_ffmpeg_progress() {
        assert_eq!(parse_progress("out_time_us=5000000", Some(10_000_000)), Some(0.5));
        assert_eq!(parse_progress("out_time_us=99000000", Some(10_000_000)), Some(0.99));
        assert_eq!(parse_progress("out_time_us=5000000", None), None);
        assert_eq!(parse_progress("progress=continue", Some(1)), None);
    }

    #[test]
    fn audio_drops_video_and_video_is_h264() {
        let args: Vec<String> = ffmpeg_args(Path::new("in.wav"), Path::new("out.mp3"), SaveFormat::Audio)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert!(args.contains(&"-vn".to_owned()) && args.contains(&"libmp3lame".to_owned()));
        assert!(args.contains(&"-n".to_owned()), "must never overwrite");
        assert_eq!(args.last().unwrap(), "out.mp3");
        let video: Vec<String> = ffmpeg_args(Path::new("in.mov"), Path::new("out.mp4"), SaveFormat::Video)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert!(video.windows(2).any(|w| w == ["-c:v", "libx264"]));
    }

    /// Needs the ffmpeg installed by the app. `-- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_wav_to_mp3_and_mp4_to_mp4() {
        let paths = ToolPaths::from_env().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        let mp4 = dir.path().join("clip.mp4");
        for (out, args) in [
            (&wav, vec!["-f", "lavfi", "-i", "sine=d=2"]),
            (&mp4, vec!["-f", "lavfi", "-i", "testsrc=d=2:s=320x240", "-c:v", "libx264"]),
        ] {
            let made = process::run(process::command(paths.ffmpeg()).args(["-nostdin", "-v", "error"]).args(args).arg(out), PROBE_TIMEOUT)
                .await
                .unwrap();
            assert!(made.status.success());
        }
        let fallback = dir.path().join("fallback");
        let progress = std::sync::Mutex::new(Vec::new());
        let report = |p: Option<f64>| progress.lock().unwrap().push(p);
        for (source, format, expected) in [(&wav, SaveFormat::Audio, "tone.mp3"), (&mp4, SaveFormat::Video, "clip (轉檔).mp4")] {
            let output = output_path(source, format, &fallback);
            let done = run_convert(&paths, source, &output, format, &CancellationToken::new(), &report).await.unwrap();
            assert!(done.ends_with(expected), "{done:?}");
            assert!(std::fs::metadata(&done).unwrap().len() > 0);
        }
        assert!(progress.lock().unwrap().iter().any(Option::is_some));
    }
}
