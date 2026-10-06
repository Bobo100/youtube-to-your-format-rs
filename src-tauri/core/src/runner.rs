//! The real queue runner: yt-dlp download, then make sure a video is H.264.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::convert::{self, ConvertError};
use crate::media::{self, MediaError};
use crate::naming::{sanitize, unique_base};
use crate::queue::{Job, JobKind, RunFuture, RunOutcome, RunUpdate, Runner, SaveFormat};
use crate::applog;
use crate::tools::update::Updater;
use crate::tools::{retry_io, ToolPaths};
use crate::ytdlp::download::{run_download, DownloadError, Update};

pub struct YtDlpRunner {
    pub paths: ToolPaths,
    pub updater: Arc<Updater>,
    pub output_dir: Box<dyn Fn() -> PathBuf + Send + Sync>,
}

impl Runner for YtDlpRunner {
    fn run<'a>(
        &'a self,
        job: &'a Job,
        cancel: &'a CancellationToken,
        update: &'a (dyn Fn(RunUpdate) + Send + Sync),
    ) -> RunFuture<'a> {
        Box::pin(async move {
            // Canceled while queued: spawning yt-dlp just to kill it would waste a start.
            if cancel.is_cancelled() {
                return RunOutcome::Canceled;
            }
            if job.kind == JobKind::Convert {
                return self.convert(job, cancel, update).await;
            }
            let dir = (self.output_dir)();
            if let Err(err) = tokio::fs::create_dir_all(&dir).await {
                applog!("cannot create {}: {err}", dir.display());
                let err = DownloadError::Io(err);
                return RunOutcome::Failed { code: err.code(), detail: err.detail() };
            }
            let base = unique_base(&dir, &sanitize(&job.title));
            let outcome = self.download(job, &dir, &base, cancel, update).await;
            if !matches!(outcome, RunOutcome::Done(_)) {
                remove_job_files(&dir, &base).await;
            }
            outcome
        })
    }
}

impl YtDlpRunner {
    /// Never uses `remove_job_files`: the source shares the output's name prefix,
    /// so only the single output file may be deleted (run_convert does that).
    async fn convert(&self, job: &Job, cancel: &CancellationToken, update: &(dyn Fn(RunUpdate) + Send + Sync)) -> RunOutcome {
        let source = PathBuf::from(&job.url);
        let output = convert::output_path(&source, job.format, &(self.output_dir)());
        if let Some(dir) = output.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        let report = |p: Option<f64>| match p {
            Some(p) => update(RunUpdate::Progress(p)),
            None => update(RunUpdate::Processing),
        };
        match convert::run_convert(&self.paths, &source, &output, job.format, cancel, &report).await {
            Ok(path) => RunOutcome::Done(path),
            Err(ConvertError::Canceled) => RunOutcome::Canceled,
            Err(err) => {
                applog!("converting {} failed: {}", source.display(), err.code());
                RunOutcome::Failed { code: err.code(), detail: err.to_string() }
            }
        }
    }

    async fn download(
        &self,
        job: &Job,
        dir: &Path,
        base: &str,
        cancel: &CancellationToken,
        update: &(dyn Fn(RunUpdate) + Send + Sync),
    ) -> RunOutcome {
        let forward = |u: Update| match u {
            Update::Progress(p) => update(RunUpdate::Progress(p)),
            Update::Processing => update(RunUpdate::Processing),
        };
        let attempt = || async {
            let _running = self.updater.running().await;
            let result = run_download(&self.paths, &job.url, job.format, dir, base, cancel, &forward).await;
            if result.is_err() {
                // A retry after an update reuses the same name; start clean.
                remove_job_files(dir, base).await;
            }
            result
        };
        let result = self.updater.with_repair(attempt, &|| update(RunUpdate::Updating), Some(cancel)).await;
        if cancel.is_cancelled() {
            return RunOutcome::Canceled;
        }
        let path = match result {
            Ok(path) => path,
            Err(DownloadError::Canceled) => return RunOutcome::Canceled,
            Err(err) => {
                applog!("download {} failed: {}", job.url, err.code());
                return RunOutcome::Failed { code: err.code(), detail: err.detail() };
            }
        };
        if job.format != SaveFormat::Video {
            return RunOutcome::Done(path);
        }
        update(RunUpdate::Processing);
        // `--recode-video` only looks at the container (VP9 inside mp4 passes),
        // so check the codec and convert only when needed.
        match media::video_codec(&self.paths, &path).await {
            Ok(codec) if codec == "h264" => RunOutcome::Done(path),
            Ok(_) => match media::transcode_to_h264(&self.paths, &path, cancel).await {
                Ok(converted) => RunOutcome::Done(converted),
                Err(MediaError::Canceled) => RunOutcome::Canceled,
                Err(err) => {
                    applog!("converting {} failed: {err}", path.display());
                    RunOutcome::Failed { code: media_code(&err), detail: err.to_string() }
                }
            },
            // The file is already downloaded and usually H.264 anyway: keep it.
            Err(err) => {
                applog!("ffprobe on {} failed, keeping the file: {err}", path.display());
                RunOutcome::Done(path)
            }
        }
    }
}

fn media_code(err: &MediaError) -> &'static str {
    match err {
        MediaError::Io(e) => DownloadError::Io(std::io::Error::new(e.kind(), e.to_string())).code(),
        _ => "extractor",
    }
}

/// Deletes every file of an unfinished job (`.part`, `.ytdl`, per-stream files).
/// Safe because `unique_base` picked a prefix no existing file had. Retries
/// because a just-killed child may still be releasing its handles.
pub async fn remove_job_files(dir: &Path, base: &str) {
    let (dir, prefix) = (dir.to_owned(), format!("{}.", base.to_lowercase()));
    let _ = tokio::task::spawn_blocking(move || {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            if entry.file_name().to_string_lossy().to_lowercase().starts_with(&prefix) {
                let _ = retry_io(|| std::fs::remove_file(entry.path()));
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::{JobState, Queue, Request};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[tokio::test]
    async fn cleanup_removes_only_the_jobs_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["歌 (2).f133.mp4.part", "歌 (2).f140.m4a", "歌 (2).ytdl", "歌.mp3", "歌 (2)x.mp3"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        remove_job_files(dir.path(), "歌 (2)").await;
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["歌 (2)x.mp3", "歌.mp3"]);
    }

    fn real_queue(dir: PathBuf) -> (Queue, Arc<Mutex<Vec<Job>>>) {
        let paths = ToolPaths::from_env().unwrap();
        let runner = YtDlpRunner {
            updater: Updater::new(paths.clone(), crate::tools::http_client().unwrap()),
            paths,
            output_dir: Box::new(move || dir.clone()),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let (queue, worker) = Queue::new(Arc::new(runner), move |job| sink.lock().unwrap().push(job.clone()));
        tokio::spawn(worker);
        (queue, events)
    }

    async fn settle(queue: &Queue, limit: Duration) {
        let start = std::time::Instant::now();
        while queue.has_active() {
            assert!(start.elapsed() < limit, "timed out: {:?}", queue.jobs());
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Real downloads of "Me at the zoo" (19 s, the first YouTube video).
    /// Needs tools installed by the app. `npm run rs:test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_audio_and_video_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let (queue, events) = real_queue(dir.path().to_owned());
        let request = |format| Request {
            video_id: "jNQXAC9IVRw".into(),
            url: "https://www.youtube.com/watch?v=jNQXAC9IVRw".into(),
            title: "Me at the zoo: 100% 原版".into(),
            format,
            ..Request::default()
        };
        queue.enqueue(vec![request(SaveFormat::Audio), request(SaveFormat::Video)], false);
        settle(&queue, Duration::from_secs(300)).await;

        let jobs = queue.jobs();
        assert!(jobs.iter().all(|j| j.state == JobState::Done), "{jobs:?}");
        assert!(jobs[0].output_path.as_deref().unwrap().ends_with("Me at the zoo_ 100% 原版.mp3"));
        // Same title, so the video gets its own prefix (see `unique_base`).
        assert!(jobs[1].output_path.as_deref().unwrap().ends_with("Me at the zoo_ 100% 原版 (2).mp4"));
        let paths = ToolPaths::from_env().unwrap();
        let video = PathBuf::from(jobs[1].output_path.clone().unwrap());
        assert_eq!(media::video_codec(&paths, &video).await.unwrap(), "h264");
        assert!(events.lock().unwrap().iter().any(|j| j.state == JobState::Processing));
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "leftover files: {names:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn real_cancel_leaves_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let (queue, events) = real_queue(dir.path().to_owned());
        // A long video, so it is still downloading when cancelled.
        let ids = queue.enqueue(vec![Request {
            video_id: "jfKfPfyJRdk".into(),
            url: "https://www.youtube.com/watch?v=aqz-KE-bpKQ".into(),
            title: "big buck bunny".into(),
            format: SaveFormat::Video,
            ..Request::default()
        }], false);
        let start = std::time::Instant::now();
        while !events.lock().unwrap().iter().any(|j| j.progress.is_some_and(|p| p > 0.01)) {
            assert!(start.elapsed() < Duration::from_secs(60), "download never started: {:?}", queue.jobs());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        queue.cancel(ids[0]);
        settle(&queue, Duration::from_secs(30)).await;
        assert_eq!(queue.jobs()[0].state, JobState::Canceled);
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert!(left.is_empty(), "leftover files: {left:?}");
    }
}
