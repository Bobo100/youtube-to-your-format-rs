use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode};
use tokio::io::AsyncWriteExt;

use super::checksum::sha256_file;
use super::install::retry_io;
use super::ToolError;

const ATTEMPTS: u32 = 3;

/// The partial file is keyed by the expected hash, so a leftover from another
/// version (after a pin bump or a new yt-dlp release) is never resumed.
pub fn part_path(dest: &Path, sha256: &str) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(format!(".{}.part", &sha256[..sha256.len().min(8)]));
    PathBuf::from(name)
}

/// Retries transient failures with backoff; permanent ones (4xx) fail at once.
pub async fn with_retry<T, F, Fut>(mut op: F) -> Result<T, ToolError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ToolError>>,
{
    let mut attempt = 0;
    loop {
        match op().await {
            Err(err) if err.is_transient() && attempt + 1 < ATTEMPTS => {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
            }
            other => return other,
        }
    }
}

/// Downloads `url` to `dest`, resuming a previous partial file, and only moves
/// it into place once its SHA-256 matches. A mismatch deletes the partial file
/// so the next attempt starts clean.
pub async fn download_verified(
    client: &Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    on_progress: &(dyn Fn(u64, Option<u64>) + Sync),
) -> Result<(), ToolError> {
    remove_stale_parts(dest, sha256).await;
    with_retry(|| try_once(client, url, dest, sha256, on_progress)).await
}

async fn remove_stale_parts(dest: &Path, sha256: &str) {
    let (Some(dir), Some(name)) = (dest.parent(), dest.file_name()) else {
        return;
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let keep = part_path(dest, sha256);
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with(&prefix) && file_name.ends_with(".part") && entry.path() != keep {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

fn content_range_start(response: &reqwest::Response) -> Option<u64> {
    let value = response.headers().get(header::CONTENT_RANGE)?.to_str().ok()?;
    let range = value.strip_prefix("bytes ")?;
    range.split('-').next()?.trim().parse().ok()
}

async fn try_once(
    client: &Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    on_progress: &(dyn Fn(u64, Option<u64>) + Sync),
) -> Result<(), ToolError> {
    let part = part_path(dest, sha256);
    let mut offset = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(header::RANGE, format!("bytes={offset}-"));
    }
    let response = request.send().await?;

    let append = match response.status() {
        StatusCode::PARTIAL_CONTENT => {
            if content_range_start(&response) != Some(offset) {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(ToolError::ResumeMismatch);
            }
            true
        }
        StatusCode::RANGE_NOT_SATISFIABLE => {
            // The partial file is already complete (or bogus); let the hash decide.
            return finish(&part, dest, sha256).await;
        }
        status if status.is_success() => {
            offset = 0;
            false
        }
        status => return Err(ToolError::Http(status.as_u16())),
    };
    let total = response.content_length().map(|len| len + offset);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&part)
        .await?;
    let mut received = offset;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        on_progress(received, total);
    }
    file.sync_all().await?;
    drop(file);

    finish(&part, dest, sha256).await
}

async fn finish(part: &Path, dest: &Path, sha256: &str) -> Result<(), ToolError> {
    let (part, dest) = (part.to_owned(), dest.to_owned());
    let sha256 = sha256.to_owned();
    tokio::task::spawn_blocking(move || {
        let actual = sha256_file(&part)?;
        if !actual.eq_ignore_ascii_case(&sha256) {
            let _ = std::fs::remove_file(&part);
            return Err(ToolError::Checksum {
                file: dest.display().to_string(),
            });
        }
        retry_io(|| std::fs::rename(&part, &dest))?;
        Ok(())
    })
    .await
    .map_err(|e| ToolError::Io(std::io::Error::other(e)))?
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn part_path_is_keyed_by_hash() {
        assert_eq!(
            part_path(Path::new(r"C:\bin\deno.zip"), ABC_SHA),
            PathBuf::from(r"C:\bin\deno.zip.ba7816bf.part")
        );
    }

    #[tokio::test]
    async fn stale_parts_of_other_versions_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("ffmpeg.7z");
        let stale = part_path(&dest, &"1".repeat(64));
        let current = part_path(&dest, ABC_SHA);
        let unrelated = dir.path().join("deno.zip.11111111.part");
        for f in [&stale, &current, &unrelated] {
            std::fs::write(f, b"x").unwrap();
        }
        remove_stale_parts(&dest, ABC_SHA).await;
        assert!(!stale.exists() && current.exists() && unrelated.exists());
    }

    #[tokio::test]
    async fn permanent_errors_are_not_retried() {
        let calls = std::sync::atomic::AtomicU32::new(0);
        let result: Result<(), _> = with_retry(|| async {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Err(ToolError::Http(404))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn finish_rejects_wrong_hash_and_removes_partial() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("x.bin");
        let wrong = "0".repeat(64);
        let part = part_path(&dest, &wrong);
        std::fs::write(&part, b"abc").unwrap();
        let err = finish(&part, &dest, &wrong).await.unwrap_err();
        assert!(matches!(err, ToolError::Checksum { .. }));
        assert!(!part.exists() && !dest.exists());
    }

    #[tokio::test]
    async fn finish_moves_verified_file_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("x.bin");
        let part = part_path(&dest, ABC_SHA);
        std::fs::write(&part, b"abc").unwrap();
        finish(&part, &dest, ABC_SHA).await.unwrap();
        assert!(dest.exists() && !part.exists());
    }

    /// Real download (~35 MB). Proves a partial file is resumed with a Range request.
    #[tokio::test]
    #[ignore]
    async fn resumes_a_partial_download() {
        use super::super::manifest::FFMPEG;
        use std::sync::atomic::{AtomicU64, Ordering};

        let client = super::super::http_client().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("ffmpeg.7z");
        const HEAD: u64 = 3 * 1024 * 1024;
        let head = client
            .get(FFMPEG.url)
            .header(header::RANGE, format!("bytes=0-{}", HEAD - 1))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(head.len() as u64, HEAD);
        std::fs::write(part_path(&dest, FFMPEG.sha256), &head).unwrap();

        let first_report = AtomicU64::new(u64::MAX);
        download_verified(&client, FFMPEG.url, &dest, FFMPEG.sha256, &|received, _| {
            let _ = first_report.compare_exchange(u64::MAX, received, Ordering::Relaxed, Ordering::Relaxed);
        })
        .await
        .unwrap();
        assert!(first_report.load(Ordering::Relaxed) > HEAD, "download restarted from zero");
        assert!(dest.exists());
    }
}
