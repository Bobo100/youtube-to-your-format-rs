//! One-at-a-time download queue. Every state change is emitted as a full Job
//! snapshot (`job-updated`), so a missed event never leaves the UI out of sync.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub use crate::ytdlp::download::SaveFormat;

pub type JobId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Downloading,
    Processing,
    Done,
    Failed,
    Canceled,
}

impl JobState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::Processing)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: JobId,
    /// Bumped on every change. Events can arrive out of order (cancel runs on a
    /// command thread, the worker on another), so the UI keeps the highest rev.
    pub rev: u64,
    pub video_id: String,
    pub url: String,
    pub title: String,
    pub format: SaveFormat,
    pub state: JobState,
    /// 0.0–1.0 while downloading; `None` when there is nothing to measure.
    pub progress: Option<f64>,
    pub output_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub video_id: String,
    pub url: String,
    pub title: String,
    pub format: SaveFormat,
}

pub enum RunUpdate {
    Progress(f64),
    Processing,
}

pub enum RunOutcome {
    Done(PathBuf),
    Failed(&'static str),
    Canceled,
}

pub type RunFuture<'a> = Pin<Box<dyn Future<Output = RunOutcome> + Send + 'a>>;

/// Does the actual work for one job; the real one drives yt-dlp + ffmpeg.
pub trait Runner: Send + Sync + 'static {
    fn run<'a>(
        &'a self,
        job: &'a Job,
        cancel: &'a CancellationToken,
        update: &'a (dyn Fn(RunUpdate) + Send + Sync),
    ) -> RunFuture<'a>;
}

type Emit = dyn Fn(&Job) + Send + Sync;

struct Inner {
    jobs: Mutex<Vec<Job>>,
    cancels: Mutex<HashMap<JobId, CancellationToken>>,
    next_id: AtomicU64,
    tx: mpsc::UnboundedSender<JobId>,
    emit: Box<Emit>,
}

#[derive(Clone)]
pub struct Queue {
    inner: Arc<Inner>,
}

impl Queue {
    /// Returns the queue and its worker; the caller spawns the worker on its runtime.
    pub fn new(
        runner: Arc<dyn Runner>,
        emit: impl Fn(&Job) + Send + Sync + 'static,
    ) -> (Self, impl Future<Output = ()> + Send + 'static) {
        let (tx, rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            jobs: Mutex::new(Vec::new()),
            cancels: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            tx,
            emit: Box::new(emit),
        });
        let worker = work(Arc::clone(&inner), runner, rx);
        (Self { inner }, worker)
    }

    /// An item already queued or running in the same format is not added again
    /// (its id is returned instead); with `skip_done`, neither is one already saved.
    pub fn enqueue(&self, requests: Vec<Request>, skip_done: bool) -> Vec<JobId> {
        requests
            .into_iter()
            .map(|request| {
                // One lock for check + insert, so two clicks racing on command
                // threads cannot both add the same item.
                let mut jobs = self.inner.jobs.lock().unwrap();
                let existing = jobs.iter().rev().find_map(|job| {
                    let same = job.video_id == request.video_id && job.format == request.format;
                    let keep = job.state.is_active() || (skip_done && job.state == JobState::Done);
                    (same && keep).then_some(job.id)
                });
                if let Some(id) = existing {
                    return id;
                }
                let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
                let job = Job {
                    id,
                    rev: 0,
                    video_id: request.video_id,
                    url: request.url,
                    title: request.title,
                    format: request.format,
                    state: JobState::Queued,
                    progress: None,
                    output_path: None,
                    error: None,
                };
                self.inner.cancels.lock().unwrap().insert(id, CancellationToken::new());
                (self.inner.emit)(&job);
                jobs.push(job);
                drop(jobs);
                let _ = self.inner.tx.send(id);
                id
            })
            .collect()
    }

    /// Queued jobs are marked canceled at once; a running one is killed by its runner.
    pub fn cancel(&self, id: JobId) {
        if let Some(token) = self.inner.cancels.lock().unwrap().get(&id) {
            token.cancel();
        }
        // Only a queued job changes here; a running one reports Canceled itself.
        self.inner.update_and_emit_if(id, |job| {
            let queued = job.state == JobState::Queued;
            if queued {
                job.state = JobState::Canceled;
            }
            queued
        });
    }

    pub fn jobs(&self) -> Vec<Job> {
        self.inner.jobs.lock().unwrap().clone()
    }

    pub fn has_active(&self) -> bool {
        self.inner.jobs.lock().unwrap().iter().any(|j| j.state.is_active())
    }
}

impl Inner {
    /// Applies `change`; when it reports a change, bumps `rev` and emits while
    /// still holding the lock so snapshots leave in the order they were made.
    fn update_and_emit_if(&self, id: JobId, change: impl FnOnce(&mut Job) -> bool) -> Option<Job> {
        let mut jobs = self.jobs.lock().unwrap();
        let job = jobs.iter_mut().find(|j| j.id == id)?;
        if change(job) {
            job.rev += 1;
            (self.emit)(job);
        }
        Some(job.clone())
    }

    fn update_and_emit(&self, id: JobId, change: impl FnOnce(&mut Job)) -> Option<Job> {
        self.update_and_emit_if(id, |job| {
            change(job);
            true
        })
    }
}

async fn work(inner: Arc<Inner>, runner: Arc<dyn Runner>, mut rx: mpsc::UnboundedReceiver<JobId>) {
    while let Some(id) = rx.recv().await {
        let Some(job) = inner.update_and_emit_if(id, |job| {
            let queued = job.state == JobState::Queued;
            if queued {
                job.state = JobState::Downloading;
                job.progress = Some(0.0);
            }
            queued
        }) else {
            continue;
        };
        if job.state != JobState::Downloading {
            continue; // canceled while queued
        }
        let token = inner.cancels.lock().unwrap().get(&id).cloned().unwrap_or_default();
        let update_inner = Arc::clone(&inner);
        let update = move |u: RunUpdate| {
            update_inner.update_and_emit(id, |job| match u {
                RunUpdate::Progress(p) => {
                    job.state = JobState::Downloading;
                    job.progress = Some(p);
                }
                RunUpdate::Processing => {
                    job.state = JobState::Processing;
                    job.progress = None;
                }
            });
        };
        let outcome = runner.run(&job, &token, &update).await;
        inner.update_and_emit(id, |job| {
            job.progress = None;
            match outcome {
                RunOutcome::Done(path) => {
                    job.state = JobState::Done;
                    job.output_path = Some(path.display().to_string());
                }
                RunOutcome::Failed(code) => {
                    job.state = JobState::Failed;
                    job.error = Some(code.to_owned());
                }
                RunOutcome::Canceled => job.state = JobState::Canceled,
            }
        });
        inner.cancels.lock().unwrap().remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Finishes after a short wait unless cancelled; titles starting "fail" fail.
    struct FakeRunner;

    impl Runner for FakeRunner {
        fn run<'a>(
            &'a self,
            job: &'a Job,
            cancel: &'a CancellationToken,
            update: &'a (dyn Fn(RunUpdate) + Send + Sync),
        ) -> RunFuture<'a> {
            Box::pin(async move {
                update(RunUpdate::Progress(0.5));
                tokio::select! {
                    _ = cancel.cancelled() => return RunOutcome::Canceled,
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                }
                if job.title.starts_with("fail") {
                    return RunOutcome::Failed("download_failed");
                }
                update(RunUpdate::Processing);
                RunOutcome::Done(PathBuf::from(format!(r"C:\out\{}.mp3", job.title)))
            })
        }
    }

    fn request(title: &str) -> Request {
        Request {
            video_id: title.into(),
            url: format!("https://youtu.be/{title}"),
            title: title.into(),
            format: SaveFormat::Audio,
        }
    }

    fn start() -> (Queue, Arc<Mutex<Vec<Job>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let (queue, worker) = Queue::new(Arc::new(FakeRunner), move |job| sink.lock().unwrap().push(job.clone()));
        tokio::spawn(worker);
        (queue, events)
    }

    async fn settle(queue: &Queue) {
        for _ in 0..200 {
            if !queue.has_active() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("queue did not settle: {:?}", queue.jobs());
    }

    #[tokio::test]
    async fn runs_jobs_one_at_a_time_in_order() {
        let (queue, events) = start();
        queue.enqueue(vec![request("a"), request("b")], false);
        settle(&queue).await;
        let events = events.lock().unwrap();
        let started: Vec<&str> = events
            .iter()
            .filter(|j| j.state == JobState::Downloading && j.progress == Some(0.0))
            .map(|j| j.title.as_str())
            .collect();
        assert_eq!(started, ["a", "b"]);
        let a_done = events.iter().position(|j| j.title == "a" && j.state == JobState::Done).unwrap();
        let b_start = events.iter().position(|j| j.title == "b" && j.state == JobState::Downloading).unwrap();
        assert!(a_done < b_start, "b started before a finished");
        assert!(queue.jobs().iter().all(|j| j.state == JobState::Done));
        assert_eq!(queue.jobs()[0].output_path.as_deref(), Some(r"C:\out\a.mp3"));
    }

    #[tokio::test]
    async fn cancel_queued_job_never_runs_it() {
        let (queue, events) = start();
        let ids = queue.enqueue(vec![request("a"), request("b")], false);
        queue.cancel(ids[1]);
        settle(&queue).await;
        assert_eq!(queue.jobs()[1].state, JobState::Canceled);
        assert!(!events.lock().unwrap().iter().any(|j| j.title == "b" && j.state == JobState::Downloading));
    }

    #[tokio::test]
    async fn cancel_running_job_stops_it_and_the_next_one_runs() {
        let (queue, _) = start();
        let ids = queue.enqueue(vec![request("a"), request("b")], false);
        tokio::time::sleep(Duration::from_millis(10)).await;
        queue.cancel(ids[0]);
        settle(&queue).await;
        let jobs = queue.jobs();
        assert_eq!(jobs[0].state, JobState::Canceled);
        assert_eq!(jobs[1].state, JobState::Done);
    }

    #[tokio::test]
    async fn same_item_is_not_queued_twice_and_batches_skip_saved_ones() {
        let (queue, _) = start();
        let first = queue.enqueue(vec![request("a")], false);
        assert_eq!(queue.enqueue(vec![request("a")], false), first, "double click queued it again");
        settle(&queue).await;
        assert_eq!(queue.enqueue(vec![request("a")], true), first, "batch re-downloaded a saved item");
        let again = queue.enqueue(vec![request("a")], false);
        assert_ne!(again, first, "a single click may save another copy");
        settle(&queue).await;
        assert_eq!(queue.jobs().len(), 2);
    }

    #[tokio::test]
    async fn every_emitted_snapshot_has_a_higher_rev() {
        let (queue, events) = start();
        let ids = queue.enqueue(vec![request("a")], false);
        settle(&queue).await;
        queue.cancel(ids[0]); // finished job: nothing changes, nothing is emitted
        let revs: Vec<u64> = events.lock().unwrap().iter().map(|j| j.rev).collect();
        assert!(revs.windows(2).all(|w| w[0] < w[1]), "{revs:?}");
        assert_eq!(*revs.last().unwrap(), queue.jobs()[0].rev);
    }

    #[tokio::test]
    async fn failure_carries_its_error_code_and_does_not_stop_the_queue() {
        let (queue, _) = start();
        queue.enqueue(vec![request("fail-1"), request("ok")], false);
        settle(&queue).await;
        let jobs = queue.jobs();
        assert_eq!((jobs[0].state, jobs[0].error.as_deref()), (JobState::Failed, Some("download_failed")));
        assert_eq!(jobs[1].state, JobState::Done);
    }
}
