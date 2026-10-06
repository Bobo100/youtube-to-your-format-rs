//! Local file conversion: anything ffmpeg can read → MP3 or MP4.
//!
//! ffmpeg writes to a reserved `.ytf-part` file; only a finished conversion is
//! linked to its final name, never replacing an existing file. So a crash or
//! cancel never leaves a clean-looking but broken `歌.mp3` next to the source,
//! and nothing the user already has can be overwritten.

use std::fs::OpenOptions;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::naming::sanitize;
use crate::process::{self, SpawnError};
use crate::queue::SaveFormat;
use crate::tools::ToolPaths;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const STDERR_LINES: usize = 40;

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("tool: {0}")]
    Spawn(#[from] SpawnError),
    #[error("ffmpeg failed: {0}")]
    Failed(String),
    #[error("source is a folder")]
    Folder,
    #[error("source has no audio")]
    NoAudio,
    #[error("canceled")]
    Canceled,
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

impl ConvertError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Spawn(SpawnError::Blocked(_)) => "tool_blocked",
            Self::Spawn(SpawnError::Missing(_)) => "tools_missing",
            Self::Io(e) if e.kind() == ErrorKind::StorageFull => "disk_full",
            Self::Io(e) if e.kind() == ErrorKind::NotFound => "source_missing",
            Self::Folder => "is_folder",
            Self::NoAudio => "no_audio",
            Self::Canceled => "canceled",
            _ => "convert_failed",
        }
    }
}

/// A claimed working file plus the name it should end up with.
#[derive(Debug)]
pub struct Reservation {
    pub part: PathBuf,
    dir: PathBuf,
    wanted: String,
    ext: &'static str,
}

/// Output naming:
/// - next to the source, same name with the new extension: `歌.wav` → `歌.mp3`
/// - already in the target format: `歌 (轉檔).mp4`
/// - a name collision adds ` (2)` (decided at `finish`, atomically)
///
/// The working file is created with `create_new` in the source folder; if that
/// folder cannot be written (a CD, a read-only share) it goes to `fallback_dir`.
pub fn reserve(source: &Path, format: SaveFormat, fallback_dir: &Path) -> io::Result<Reservation> {
    let stem = sanitize(&source.file_stem().unwrap_or_default().to_string_lossy());
    let same_format = source.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension()));
    let wanted = if same_format { format!("{stem} (轉檔)") } else { stem };
    let ext = format.extension();
    let mut last_err = io::Error::other("no folder to write to");
    for dir in source.parent().into_iter().chain([fallback_dir]) {
        if dir == fallback_dir {
            std::fs::create_dir_all(dir)?;
        }
        for n in 1..=100 {
            let part = dir.join(format!("{wanted}.{ext}.{n}.ytf-part"));
            match OpenOptions::new().write(true).create_new(true).open(&part) {
                Ok(_) => return Ok(Reservation { part, dir: dir.to_owned(), wanted: wanted.clone(), ext }),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    last_err = e;
                    break;
                }
            }
        }
    }
    Err(last_err)
}

impl Reservation {
    /// Gives the finished file its final name without ever replacing a file.
    pub fn finish(self) -> io::Result<PathBuf> {
        for n in 1.. {
            let name = if n == 1 { self.wanted.clone() } else { format!("{} ({n})", self.wanted) };
            let target = self.dir.join(format!("{name}.{}", self.ext));
            match std::fs::hard_link(&self.part, &target) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&self.part);
                    return Ok(target);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                // FAT32 USB sticks have no hard links; fall back to a checked rename.
                Err(_) if !target.exists() => {
                    std::fs::rename(&self.part, &target)?;
                    return Ok(target);
                }
                Err(_) => continue,
            }
        }
        unreachable!("an unbounded range always finds a free name")
    }

    pub fn discard(self) {
        let _ = std::fs::remove_file(&self.part);
    }
}

pub fn ffmpeg_args(source: &Path, output: &Path, format: SaveFormat) -> Vec<std::ffi::OsString> {
    // -y is safe: `output` is our own reserved, empty working file.
    let mut args: Vec<std::ffi::OsString> =
        ["-nostdin", "-hide_banner", "-v", "error", "-nostats", "-progress", "pipe:1", "-y", "-i"]
            .map(Into::into)
            .to_vec();
    args.push(source.as_os_str().to_owned());
    let rest: &[&str] = match format {
        // Only the first audio stream; subtitles / data streams would make ffmpeg fail.
        SaveFormat::Audio => &["-map", "0:a:0", "-vn", "-sn", "-dn", "-c:a", "libmp3lame", "-q:a", "0", "-f", "mp3"],
        SaveFormat::Video => &[
            "-map", "0:v:0?", "-map", "0:a:0?", "-sn", "-dn",
            // libx264 + yuv420p needs even sizes; 853x480 videos are common.
            "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", "-f", "mp4",
        ],
    };
    args.extend(rest.iter().map(Into::into));
    args.push(output.as_os_str().to_owned());
    args
}

/// Reads `out_time_us=…` from ffmpeg's `-progress` output as a 0–1 fraction.
pub fn parse_progress(line: &str, total_us: Option<u64>) -> Option<f64> {
    let value = line.strip_prefix("out_time_us=")?.trim().parse::<u64>().ok()?;
    let total = total_us.filter(|t| *t > 0)?;
    Some((value as f64 / total as f64).clamp(0.0, 0.99))
}

#[derive(Debug, Default, PartialEq)]
pub struct Probe {
    pub duration_us: Option<u64>,
    pub has_audio: bool,
}

pub fn parse_probe(json: &str) -> Probe {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Probe::default();
    };
    let duration_us = value
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
        .map(|seconds| (seconds * 1_000_000.0) as u64);
    let has_audio = value
        .get("streams")
        .and_then(Value::as_array)
        .is_some_and(|streams| streams.iter().any(|s| s.get("codec_type").and_then(Value::as_str) == Some("audio")));
    Probe { duration_us, has_audio }
}

async fn probe(paths: &ToolPaths, source: &Path, cancel: &CancellationToken) -> Result<Probe, ConvertError> {
    let mut cmd = process::command(paths.ffprobe());
    cmd.args(["-v", "error", "-show_entries", "format=duration:stream=codec_type", "-of", "json"]).arg(source);
    tokio::select! {
        _ = cancel.cancelled() => Err(ConvertError::Canceled),
        out = process::run(&mut cmd, PROBE_TIMEOUT) => {
            Ok(out.map(|o| parse_probe(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default())
        }
    }
}

pub async fn run_convert(
    paths: &ToolPaths,
    source: &Path,
    format: SaveFormat,
    fallback_dir: &Path,
    cancel: &CancellationToken,
    on_progress: &(dyn Fn(Option<f64>) + Send + Sync),
) -> Result<PathBuf, ConvertError> {
    if source.is_dir() {
        return Err(ConvertError::Folder);
    }
    if !source.is_file() {
        return Err(ConvertError::Io(io::Error::from(ErrorKind::NotFound)));
    }
    let info = probe(paths, source, cancel).await?;
    if format == SaveFormat::Audio && !info.has_audio && info.duration_us.is_some() {
        return Err(ConvertError::NoAudio);
    }
    let reservation = reserve(source, format, fallback_dir)?;
    match encode(paths, source, &reservation.part, format, info.duration_us, cancel, on_progress).await {
        Ok(()) => Ok(reservation.finish()?),
        Err(err) => {
            reservation.discard();
            Err(err)
        }
    }
}

async fn encode(
    paths: &ToolPaths,
    source: &Path,
    output: &Path,
    format: SaveFormat,
    total: Option<u64>,
    cancel: &CancellationToken,
    on_progress: &(dyn Fn(Option<f64>) + Send + Sync),
) -> Result<(), ConvertError> {
    let mut cmd = process::command(paths.ffmpeg());
    cmd.args(ffmpeg_args(source, output, format)).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut spawned = process::spawn(&mut cmd)?;
    let stdout = spawned.child.stdout.take().expect("stdout is piped");
    let stderr = spawned.child.stderr.take().expect("stderr is piped");
    // A damaged file can print an error per frame; keep only the tail.
    let stderr_task = tokio::spawn(async move {
        let mut tail = std::collections::VecDeque::with_capacity(STDERR_LINES);
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tail.len() == STDERR_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        Vec::from(tail).join("\n")
    });
    let mut lines = BufReader::new(stdout).lines();
    let status = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                spawned.kill_tree();
                let _ = spawned.child.wait().await;
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
        return Err(ConvertError::Failed(stderr_task.await.unwrap_or_default()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(source: &Path, format: SaveFormat, fallback: &Path) -> PathBuf {
        let reservation = reserve(source, format, fallback).unwrap();
        std::fs::write(&reservation.part, b"done").unwrap();
        reservation.finish().unwrap()
    }

    #[test]
    fn output_goes_next_to_the_source_with_the_new_extension() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let source = dir.path().join("歌.wav");
        std::fs::write(&source, b"").unwrap();
        assert_eq!(finished(&source, SaveFormat::Audio, fallback.path()), dir.path().join("歌.mp3"));
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".ytf-part"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn same_format_gets_a_marked_name_and_collisions_are_numbered() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let source = dir.path().join("影片.MP4");
        std::fs::write(&source, b"original").unwrap();
        assert_eq!(finished(&source, SaveFormat::Video, fallback.path()), dir.path().join("影片 (轉檔).mp4"));
        assert_eq!(finished(&source, SaveFormat::Video, fallback.path()), dir.path().join("影片 (轉檔) (2).mp4"));
        // The source shares the `影片.` prefix but is not the output name.
        assert_eq!(finished(&source, SaveFormat::Audio, fallback.path()), dir.path().join("影片.mp3"));
        assert_eq!(std::fs::read(&source).unwrap(), b"original");
    }

    #[test]
    fn finishing_never_replaces_a_file_that_appeared_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let source = dir.path().join("歌.wav");
        std::fs::write(&source, b"").unwrap();
        let reservation = reserve(&source, SaveFormat::Audio, fallback.path()).unwrap();
        std::fs::write(dir.path().join("歌.MP3"), b"someone else").unwrap();
        std::fs::write(&reservation.part, b"ours").unwrap();
        let out = reservation.finish().unwrap();
        assert_eq!(out, dir.path().join("歌 (2).mp3"));
        assert_eq!(std::fs::read(dir.path().join("歌.MP3")).unwrap(), b"someone else");
    }

    #[test]
    fn unwritable_folder_falls_back() {
        let fallback = tempfile::tempdir().unwrap();
        let source = Path::new(r"Z:\no-such-drive\歌.wav");
        assert_eq!(finished(source, SaveFormat::Audio, fallback.path()), fallback.path().join("歌.mp3"));
    }

    #[test]
    fn discard_removes_only_the_working_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("歌.wav");
        std::fs::write(&source, b"keep").unwrap();
        let reservation = reserve(&source, SaveFormat::Audio, dir.path()).unwrap();
        reservation.discard();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["歌.wav"]);
    }

    #[test]
    fn reads_ffmpeg_progress_and_probe() {
        assert_eq!(parse_progress("out_time_us=5000000", Some(10_000_000)), Some(0.5));
        assert_eq!(parse_progress("out_time_us=99000000", Some(10_000_000)), Some(0.99));
        assert_eq!(parse_progress("out_time_us=N/A", Some(10)), None);
        assert_eq!(parse_progress("progress=continue", Some(1)), None);
        let probe = parse_probe(r#"{"streams":[{"codec_type":"video"},{"codec_type":"audio"}],"format":{"duration":"2.5"}}"#);
        assert_eq!(probe, Probe { duration_us: Some(2_500_000), has_audio: true });
        assert!(!parse_probe(r#"{"streams":[{"codec_type":"video"}],"format":{}}"#).has_audio);
    }

    #[test]
    fn audio_takes_one_audio_stream_and_video_gets_even_sizes() {
        let strings = |format, out| -> Vec<String> {
            ffmpeg_args(Path::new("in.mkv"), Path::new(out), format)
                .into_iter()
                .map(|a| a.into_string().unwrap())
                .collect()
        };
        let audio = strings(SaveFormat::Audio, "o.part");
        assert!(audio.windows(2).any(|w| w == ["-map", "0:a:0"]) && audio.contains(&"-sn".to_owned()));
        assert!(audio.windows(2).any(|w| w == ["-f", "mp3"]));
        let video = strings(SaveFormat::Video, "o.part");
        assert!(video.windows(2).any(|w| w == ["-c:v", "libx264"]));
        assert!(video.iter().any(|a| a.starts_with("scale=trunc(iw/2)*2")));
        assert_eq!(video.last().unwrap(), "o.part");
    }

    /// Needs the ffmpeg installed by the app. `-- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_conversions_including_odd_sizes_and_silent_video() {
        let paths = ToolPaths::from_env().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let fallback = dir.path().join("fallback");
        let make = |name: &str, args: &[&str]| {
            let out = dir.path().join(name);
            let paths = paths.clone();
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            async move {
                let made = process::run(
                    process::command(paths.ffmpeg()).args(["-nostdin", "-v", "error"]).args(&args).arg(&out),
                    PROBE_TIMEOUT,
                )
                .await
                .unwrap();
                assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
                out
            }
        };
        let wav = make("tone.wav", &["-f", "lavfi", "-i", "sine=d=2"]).await;
        let odd = make(
            "odd.mkv",
            &["-f", "lavfi", "-i", "testsrc=d=2:s=321x241", "-f", "lavfi", "-i", "sine=d=2", "-c:v", "libx264", "-pix_fmt", "yuv444p", "-shortest"],
        )
        .await;
        let silent = make("silent.mp4", &["-f", "lavfi", "-i", "testsrc=d=1:s=320x240", "-c:v", "libx264"]).await;

        let progress = std::sync::Mutex::new(Vec::new());
        let report = |p: Option<f64>| progress.lock().unwrap().push(p);
        let never = CancellationToken::new();
        let mp3 = run_convert(&paths, &wav, SaveFormat::Audio, &fallback, &never, &report).await.unwrap();
        assert!(mp3.ends_with("tone.mp3"));
        let mp4 = run_convert(&paths, &odd, SaveFormat::Video, &fallback, &never, &report).await.unwrap();
        assert!(mp4.ends_with("odd.mp4") && std::fs::metadata(&mp4).unwrap().len() > 0);
        let err = run_convert(&paths, &silent, SaveFormat::Audio, &fallback, &never, &report).await.unwrap_err();
        assert_eq!(err.code(), "no_audio");
        assert!(progress.lock().unwrap().iter().any(Option::is_some));
        let parts: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".ytf-part"))
            .collect();
        assert!(parts.is_empty(), "working files left behind");
    }
}
