//! ffprobe / ffmpeg work after a download: make sure a video plays everywhere.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::process::{self, SpawnError};
use crate::tools::ToolPaths;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("tool: {0}")]
    Spawn(#[from] SpawnError),
    #[error("ffmpeg failed: {0}")]
    Failed(String),
    #[error("canceled")]
    Canceled,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn video_codec(paths: &ToolPaths, file: &Path) -> Result<String, MediaError> {
    let out = process::run(
        process::command(paths.ffprobe())
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=codec_name"])
            .args(["-of", "default=nw=1:nk=1"])
            .arg(file),
        PROBE_TIMEOUT,
    )
    .await?;
    if !out.status.success() {
        return Err(MediaError::Failed(String::from_utf8_lossy(&out.stderr).into_owned()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// `C:\a\歌.mp4` → `C:\a\歌.h264.tmp.mp4`: keeps the job's `<base>.` prefix so
/// a cancelled transcode is cleaned up with the rest of the job's files.
pub fn transcode_temp(file: &Path) -> PathBuf {
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    file.with_file_name(format!("{stem}.h264.tmp.mp4"))
}

pub async fn transcode_to_h264(
    paths: &ToolPaths,
    file: &Path,
    cancel: &CancellationToken,
) -> Result<(), MediaError> {
    let temp = transcode_temp(file);
    let mut cmd = process::command(paths.ffmpeg());
    cmd.args(["-nostdin", "-y", "-v", "error", "-i"])
        .arg(file)
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"])
        .args(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"])
        .arg(&temp);
    cmd.stdout(std::process::Stdio::null());
    let mut spawned = process::spawn(&mut cmd)?;
    let stderr = read_stderr(&mut spawned);
    let status = tokio::select! {
        _ = cancel.cancelled() => None,
        status = spawned.child.wait() => Some(status?),
    };
    let Some(status) = status else {
        spawned.kill_tree();
        let _ = spawned.child.wait().await;
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(MediaError::Canceled);
    };
    if !status.success() {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(MediaError::Failed(stderr.await.unwrap_or_default()));
    }
    tokio::fs::rename(&temp, file).await?;
    Ok(())
}

fn read_stderr(spawned: &mut process::Spawned) -> tokio::task::JoinHandle<String> {
    use tokio::io::AsyncReadExt;
    let pipe = spawned.child.stderr.take();
    tokio::spawn(async move {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text).await;
        }
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the ffmpeg installed by the app. `npm run rs:test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_vp9_is_converted_to_h264() {
        let paths = ToolPaths::from_env().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vp9.mp4");
        let made = process::run(
            process::command(paths.ffmpeg())
                .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i", "testsrc=d=1:s=320x240"])
                .args(["-c:v", "libvpx-vp9"])
                .arg(&file),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        assert_eq!(video_codec(&paths, &file).await.unwrap(), "vp9");

        transcode_to_h264(&paths, &file, &CancellationToken::new()).await.unwrap();
        assert_eq!(video_codec(&paths, &file).await.unwrap(), "h264");
        assert!(!transcode_temp(&file).exists());
    }

    #[test]
    fn temp_file_keeps_the_job_prefix() {
        assert_eq!(
            transcode_temp(Path::new(r"C:\a\月亮 (2).mp4")),
            PathBuf::from(r"C:\a\月亮 (2).h264.tmp.mp4")
        );
    }
}
