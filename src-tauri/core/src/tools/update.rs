//! Keeps yt-dlp current without anyone touching the family's computer:
//! a daily background check, plus an immediate stable → nightly update when a
//! download fails the way YouTube changes make it fail.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use tokio::sync::{RwLock, RwLockReadGuard};

use super::download::with_retry;
use super::manifest::{YTDLP_NIGHTLY_REPO, YTDLP_STABLE_REPO};
use super::state::ToolState;
use super::version::is_newer;
use super::{install_ytdlp, verify_or_rollback, ytdlp_release, ToolError, ToolPaths};
use crate::applog;

const DAILY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Nightly,
}

impl Channel {
    fn repo(self) -> &'static str {
        match self {
            Self::Stable => YTDLP_STABLE_REPO,
            Self::Nightly => YTDLP_NIGHTLY_REPO,
        }
    }
}

/// Errors that a newer yt-dlp might fix.
pub trait NeedsNewYtdlp {
    fn needs_new_ytdlp(&self) -> bool;
}

pub struct Updater {
    paths: ToolPaths,
    client: Client,
    /// Readers run yt-dlp; the writer swaps its folder. Windows cannot replace
    /// files of a running exe, so a swap waits for every run to finish.
    gate: RwLock<()>,
}

impl Updater {
    pub fn new(paths: ToolPaths, client: Client) -> Arc<Self> {
        Arc::new(Self { paths, client, gate: RwLock::new(()) })
    }

    /// Hold this while yt-dlp runs.
    pub async fn running(&self) -> RwLockReadGuard<'_, ()> {
        self.gate.read().await
    }

    /// At most once a day; returns whether a new version was installed.
    pub async fn check_daily(&self) -> Result<bool, ToolError> {
        self.update(Channel::Stable, false).await
    }

    /// Installs the channel's latest release if it is newer than what is installed.
    pub async fn update(&self, channel: Channel, force: bool) -> Result<bool, ToolError> {
        let _swap = self.gate.write().await;
        tokio::fs::create_dir_all(&self.paths.bin).await?;
        let state_path = self.paths.bin.join("state.json");
        let mut state = ToolState::load(&state_path);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let recently = state.ytdlp_checked_at.is_some_and(|at| now.saturating_sub(at) < DAILY.as_secs());
        if !force && recently {
            return Ok(false);
        }
        let release = with_retry(|| ytdlp_release(&self.client, channel.repo())).await?;
        state.ytdlp_checked_at = Some(now);
        let current = state.ytdlp.clone().unwrap_or_default();
        if !is_newer(&release.version, &current) {
            state.save(&state_path)?;
            return Ok(false);
        }
        applog!("updating yt-dlp {current} -> {} ({channel:?})", release.version);
        let version = install_ytdlp(&self.paths, &self.client, &release, &|_, _, _| {}).await?;
        verify_or_rollback(&[(self.paths.ytdlp(), "--version", self.paths.ytdlp_dir())]).await?;
        state.ytdlp = Some(version);
        state.save(&state_path)?;
        Ok(true)
    }

    /// Runs `op`; if it fails in a way a newer yt-dlp might fix, updates (stable,
    /// then nightly) and tries again. `on_repair` fires before each update so the
    /// UI can say what is going on. `op` must take `running()` itself.
    pub async fn with_repair<T, E, F, Fut>(&self, op: F, on_repair: &(dyn Fn() + Sync)) -> Result<T, E>
    where
        E: NeedsNewYtdlp,
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        repair_loop(op, |channel| self.update(channel, true), on_repair).await
    }
}

/// The retry policy, separate from the network so it can be tested on its own.
async fn repair_loop<T, E, F, Fut, U, UFut>(mut op: F, mut update: U, on_repair: &(dyn Fn() + Sync)) -> Result<T, E>
where
    E: NeedsNewYtdlp,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    U: FnMut(Channel) -> UFut,
    UFut: Future<Output = Result<bool, ToolError>>,
{
    let mut result = op().await;
    for channel in [Channel::Stable, Channel::Nightly] {
        if !matches!(&result, Err(err) if err.needs_new_ytdlp()) {
            break;
        }
        on_repair();
        match update(channel).await {
            Ok(true) => result = op().await,
            Ok(false) => {}
            Err(err) => applog!("yt-dlp {channel:?} update failed: {err}"),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, PartialEq)]
    struct Broken(bool);
    impl NeedsNewYtdlp for Broken {
        fn needs_new_ytdlp(&self) -> bool {
            self.0
        }
    }

    /// Runs the loop with scripted op results and update outcomes; returns the
    /// result and the channels that were tried.
    async fn scripted(
        ops: Vec<Result<(), Broken>>,
        updates: Vec<Result<bool, ToolError>>,
    ) -> (Result<(), Broken>, Vec<Channel>) {
        let ops = Mutex::new(ops.into_iter());
        let updates = Mutex::new(updates.into_iter());
        let tried = Mutex::new(Vec::new());
        let result = repair_loop(
            || {
                let next = ops.lock().unwrap().next().expect("op called too often");
                async move { next }
            },
            |channel| {
                tried.lock().unwrap().push(channel);
                let next = updates.lock().unwrap().next().expect("update called too often");
                async move { next }
            },
            &|| {},
        )
        .await;
        (result, tried.into_inner().unwrap())
    }

    #[tokio::test]
    async fn errors_a_new_version_cannot_fix_are_not_repaired() {
        let (result, tried) = scripted(vec![Err(Broken(false))], vec![]).await;
        assert_eq!(result, Err(Broken(false)));
        assert!(tried.is_empty());
    }

    #[tokio::test]
    async fn a_stable_update_that_fixes_it_stops_there() {
        let (result, tried) = scripted(vec![Err(Broken(true)), Ok(())], vec![Ok(true)]).await;
        assert_eq!(result, Ok(()));
        assert_eq!(tried, [Channel::Stable]);
    }

    #[tokio::test]
    async fn no_newer_stable_goes_on_to_nightly() {
        let (result, tried) = scripted(vec![Err(Broken(true)), Ok(())], vec![Ok(false), Ok(true)]).await;
        assert_eq!(result, Ok(()));
        assert_eq!(tried, [Channel::Stable, Channel::Nightly]);
    }

    #[tokio::test]
    async fn a_failed_update_does_not_retry_the_download() {
        let (result, tried) =
            scripted(vec![Err(Broken(true))], vec![Err(ToolError::Http(503)), Err(ToolError::Http(503))]).await;
        assert_eq!(result, Err(Broken(true)));
        assert_eq!(tried, [Channel::Stable, Channel::Nightly]);
    }

    /// The QA scenario "YouTube changed": an outdated yt-dlp fails a real download,
    /// the runner updates it and the retry succeeds. Needs the app's installed
    /// tools (hard-linked into a temp bin) and network. `-- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_outdated_ytdlp_is_updated_and_the_download_retried() {
        use crate::queue::{JobState, Queue, Request, SaveFormat};
        use crate::runner::YtDlpRunner;

        let installed = ToolPaths::from_env().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let paths = ToolPaths { bin: bin.path().into() };
        for (from, to) in [
            (installed.ffmpeg(), paths.ffmpeg()),
            (installed.ffprobe(), paths.ffprobe()),
            (installed.deno(), paths.deno()),
        ] {
            std::fs::hard_link(from, to).unwrap();
        }
        let client = super::super::http_client().unwrap();
        let zip = bin.path().join("old.zip");
        let bytes = client
            .get("https://github.com/yt-dlp/yt-dlp/releases/download/2025.01.15/yt-dlp_win.zip")
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        std::fs::write(&zip, bytes).unwrap();
        super::super::install::extract_zip_tree(&zip, &paths.ytdlp_dir()).unwrap();
        super::super::install::replace_with_new(&paths.ytdlp_dir()).unwrap();
        ToolState { ytdlp: Some("2025.01.15".into()), ..ToolState::default() }
            .save(&bin.path().join("state.json"))
            .unwrap();

        let out_dir = out.path().to_owned();
        let runner = YtDlpRunner {
            updater: Updater::new(paths.clone(), client),
            paths: paths.clone(),
            output_dir: Box::new(move || out_dir.clone()),
        };
        let states = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&states);
        let (queue, worker) = Queue::new(Arc::new(runner), move |job| sink.lock().unwrap().push(job.state));
        tokio::spawn(worker);
        queue.enqueue(
            vec![Request {
                video_id: "jNQXAC9IVRw".into(),
                url: "https://www.youtube.com/watch?v=jNQXAC9IVRw".into(),
                title: "zoo".into(),
                format: SaveFormat::Audio,
            }],
            false,
        );
        let start = std::time::Instant::now();
        while queue.has_active() {
            assert!(start.elapsed() < Duration::from_secs(300), "{:?}", queue.jobs());
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let job = &queue.jobs()[0];
        assert_eq!(job.state, JobState::Done, "{job:?} {:?}", queue.detail(job.id));
        assert!(states.lock().unwrap().contains(&JobState::Updating));
        let now = ToolState::load(&bin.path().join("state.json")).ytdlp.unwrap();
        assert!(is_newer(&now, "2025.01.15"), "still on {now}");
    }

    /// Real network. Installs the latest stable into a temp dir, then checks
    /// that a second forced check finds nothing newer.
    #[tokio::test]
    #[ignore]
    async fn real_update_installs_then_reports_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ToolPaths { bin: dir.path().into() };
        let updater = Updater::new(paths.clone(), super::super::http_client().unwrap());
        assert!(updater.update(Channel::Stable, true).await.unwrap());
        assert!(paths.ytdlp().is_file());
        assert!(!updater.update(Channel::Stable, true).await.unwrap());
        assert!(!updater.check_daily().await.unwrap(), "checked a moment ago");
    }
}
