use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode};
use tokio::io::AsyncWriteExt;

use super::checksum::sha256_file;
use super::ToolError;

const ATTEMPTS: u32 = 3;

pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Downloads `url` to `dest`, resuming a previous `.part` file, and only moves
/// it into place once its SHA-256 matches. A mismatch deletes the partial file
/// so the next attempt starts clean.
pub async fn download_verified(
    client: &Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    on_progress: &(dyn Fn(u64, Option<u64>) + Sync),
) -> Result<(), ToolError> {
    let mut last_err = None;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
        }
        match try_once(client, url, dest, sha256, on_progress).await {
            Ok(()) => return Ok(()),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.expect("at least one attempt"))
}

async fn try_once(
    client: &Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    on_progress: &(dyn Fn(u64, Option<u64>) + Sync),
) -> Result<(), ToolError> {
    let part = part_path(dest);
    let mut offset = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(header::RANGE, format!("bytes={offset}-"));
    }
    let response = request.send().await?;

    let append = match response.status() {
        StatusCode::PARTIAL_CONTENT => true,
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
    let part_owned = part.to_owned();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&part_owned))
        .await
        .map_err(|e| ToolError::Io(std::io::Error::other(e)))??;
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = tokio::fs::remove_file(part).await;
        return Err(ToolError::Checksum {
            file: dest.display().to_string(),
        });
    }
    tokio::fs::rename(part, dest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(
            part_path(Path::new(r"C:\bin\deno.zip")),
            PathBuf::from(r"C:\bin\deno.zip.part")
        );
    }

    #[tokio::test]
    async fn finish_rejects_wrong_hash_and_removes_partial() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("x.bin");
        let part = part_path(&dest);
        std::fs::write(&part, b"abc").unwrap();
        let err = finish(&part, &dest, &"0".repeat(64)).await.unwrap_err();
        assert!(matches!(err, ToolError::Checksum { .. }));
        assert!(!part.exists() && !dest.exists());
    }

    /// Real download (~35 MB). Proves a `.part` file is resumed with a Range request.
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
        std::fs::write(part_path(&dest), &head).unwrap();

        let first_report = AtomicU64::new(u64::MAX);
        download_verified(&client, FFMPEG.url, &dest, FFMPEG.sha256, &|received, _| {
            let _ = first_report.compare_exchange(u64::MAX, received, Ordering::Relaxed, Ordering::Relaxed);
        })
        .await
        .unwrap();
        assert!(first_report.load(Ordering::Relaxed) > HEAD, "download restarted from zero");
        assert!(dest.exists());
    }

    #[tokio::test]
    async fn finish_moves_verified_file_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("x.bin");
        let part = part_path(&dest);
        std::fs::write(&part, b"abc").unwrap();
        finish(
            &part,
            &dest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        .await
        .unwrap();
        assert!(dest.exists() && !part.exists());
    }
}
