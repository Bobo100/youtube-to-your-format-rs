//! Keeps yt-dlp current without anyone touching the family's computer:
//! a daily background check, plus an immediate stable → nightly update when a
//! download fails the way YouTube changes make it fail.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use tokio::sync::{MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio_util::sync::CancellationToken;

use super::download::with_retry;
use super::manifest::{YTDLP_NIGHTLY_REPO, YTDLP_STABLE_REPO};
use super::state::ToolState;
use super::version::is_newer;
use super::{stage_ytdlp, swap_ytdlp, ytdlp_release, ToolError, ToolPaths};
use crate::applog;

const DAILY: Duration = Duration::from_secs(24 * 60 * 60);
/// A broken playlist fails every song; without this each one would ask GitHub
/// again and burn the 60 requests/hour an unauthenticated IP gets.
const FORCED_COOLDOWN: Duration = Duration::from_secs(30 * 60);
const RATE_LIMIT_PAUSE: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[derive(Default)]
struct Memo {
    last_forced: HashMap<Channel, Instant>,
    paused_until: Option<Instant>,
    /// Versions that did not run here (e.g. blocked by antivirus).
    rejected: HashSet<String>,
}

pub struct Updater {
    paths: ToolPaths,
    client: Client,
    /// Readers run yt-dlp; the writer swaps its folder. Windows cannot replace
    /// files of a running exe, so a swap waits for every run to finish — and is
    /// therefore kept as short as possible (download and unpack happen outside).
    gate: RwLock<()>,
    /// Single owner of `bin\` staging files and state.json: `prepare` and the
    /// updater both take it, so neither overwrites the other's work.
    files: tokio::sync::Mutex<()>,
    memo: Mutex<Memo>,
}

impl Updater {
    pub fn new(paths: ToolPaths, client: Client) -> Arc<Self> {
        Arc::new(Self {
            paths,
            client,
            gate: RwLock::new(()),
            files: tokio::sync::Mutex::new(()),
            memo: Mutex::new(Memo::default()),
        })
    }

    /// Hold this while yt-dlp runs.
    pub async fn running(&self) -> RwLockReadGuard<'_, ()> {
        self.gate.read().await
    }

    /// For `prepare`: the files lock, plus the write gate when it will install.
    pub async fn files(&self) -> MutexGuard<'_, ()> {
        self.files.lock().await
    }

    pub async fn exclusive(&self) -> RwLockWriteGuard<'_, ()> {
        self.gate.write().await
    }

    fn installed(&self) -> Option<String> {
        ToolState::load(&self.paths.bin.join("state.json")).ytdlp
    }

    /// At most once a day; returns whether a new version was installed.
    pub async fn check_daily(&self) -> Result<bool, ToolError> {
        self.update(Channel::Stable, false, None).await
    }

    /// Installs the channel's latest release if it is newer than what is installed.
    /// `failed_on`: the version a failed run used; if it was replaced meanwhile,
    /// returns `Ok(true)` at once so the caller retries with what is there now.
    pub async fn update(&self, channel: Channel, force: bool, failed_on: Option<&str>) -> Result<bool, ToolError> {
        let _files = self.files.lock().await;
        tokio::fs::create_dir_all(&self.paths.bin).await?;
        let state_path = self.paths.bin.join("state.json");
        let mut state = ToolState::load(&state_path);
        if failed_on.is_some() && state.ytdlp.as_deref() != failed_on {
            return Ok(true);
        }
        if !self.may_ask(channel, force, &state) {
            return Ok(false);
        }
        let release = match with_retry(|| ytdlp_release(&self.client, channel.repo())).await {
            Ok(release) => release,
            Err(err) => {
                if matches!(err, ToolError::Http(403 | 429)) {
                    self.memo.lock().unwrap().paused_until = Some(Instant::now() + RATE_LIMIT_PAUSE);
                }
                return Err(err);
            }
        };
        state.ytdlp_checked_at = Some(unix_now());
        let current = state.ytdlp.clone().unwrap_or_default();
        let rejected = self.memo.lock().unwrap().rejected.contains(&release.version);
        if rejected || !is_newer(&release.version, &current) {
            state.save(&state_path)?;
            return Ok(false);
        }
        applog!("updating yt-dlp {current} -> {} ({channel:?})", release.version);
        let version = stage_ytdlp(&self.paths, &self.client, &release, &|_, _, _| {}).await?;
        {
            let _swap = self.gate.write().await;
            if let Err(err) = swap_ytdlp(&self.paths).await {
                self.memo.lock().unwrap().rejected.insert(version);
                state.save(&state_path)?;
                return Err(err);
            }
        }
        state.ytdlp = Some(version);
        state.save(&state_path)?;
        Ok(true)
    }

    fn may_ask(&self, channel: Channel, force: bool, state: &ToolState) -> bool {
        let mut memo = self.memo.lock().unwrap();
        let now = Instant::now();
        if memo.paused_until.is_some_and(|until| now < until) {
            return false;
        }
        if !force {
            return !state.ytdlp_checked_at.is_some_and(|at| unix_now().saturating_sub(at) < DAILY.as_secs());
        }
        if memo.last_forced.get(&channel).is_some_and(|at| now.duration_since(*at) < FORCED_COOLDOWN) {
            return false;
        }
        memo.last_forced.insert(channel, now);
        true
    }

    /// Runs `op`; if it fails in a way a newer yt-dlp might fix, updates (stable,
    /// then nightly) and tries again. `on_repair` fires before each update so the
    /// UI can say what is going on. `op` must take `running()` itself. A cancel
    /// stops waiting at once; an install already under way finishes in the background
    /// only as far as its current step.
    pub async fn with_repair<T, E, F, Fut>(
        &self,
        op: F,
        on_repair: &(dyn Fn() + Sync),
        cancel: Option<&CancellationToken>,
    ) -> Result<T, E>
    where
        E: NeedsNewYtdlp,
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let never = CancellationToken::new();
        let cancel = cancel.unwrap_or(&never);
        repair_loop(
            op,
            || self.installed(),
            |channel, failed_on| async move {
                tokio::select! {
                    _ = cancel.cancelled() => Ok(false),
                    result = self.update(channel, true, failed_on.as_deref()) => result,
                }
            },
            on_repair,
            cancel,
        )
        .await
    }
}

/// The retry policy, separate from the network so it can be tested on its own.
async fn repair_loop<T, E, F, Fut, V, U, UFut>(
    mut op: F,
    version: V,
    mut update: U,
    on_repair: &(dyn Fn() + Sync),
    cancel: &CancellationToken,
) -> Result<T, E>
where
    E: NeedsNewYtdlp,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    V: Fn() -> Option<String>,
    U: FnMut(Channel, Option<String>) -> UFut,
    UFut: Future<Output = Result<bool, ToolError>>,
{
    let mut used = version();
    let mut result = op().await;
    for channel in [Channel::Stable, Channel::Nightly] {
        if cancel.is_cancelled() || !matches!(&result, Err(err) if err.needs_new_ytdlp()) {
            break;
        }
        on_repair();
        match update(channel, used.clone()).await {
            Ok(true) if !cancel.is_cancelled() => {
                used = version();
                result = op().await;
            }
            Ok(_) => {}
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
            || None,
            |channel, _failed_on| {
                tried.lock().unwrap().push(channel);
                let next = updates.lock().unwrap().next().expect("update called too often");
                async move { next }
            },
            &|| {},
            &CancellationToken::new(),
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

    #[test]
    fn forced_checks_cool_down_and_rate_limits_pause_everything() {
        let updater = Updater::new(ToolPaths { bin: "unused".into() }, Client::new());
        let state = ToolState::default();
        assert!(updater.may_ask(Channel::Stable, true, &state));
        assert!(!updater.may_ask(Channel::Stable, true, &state), "asked again within the cooldown");
        assert!(updater.may_ask(Channel::Nightly, true, &state), "channels cool down separately");
        updater.memo.lock().unwrap().paused_until = Some(Instant::now() + RATE_LIMIT_PAUSE);
        assert!(!updater.may_ask(Channel::Stable, false, &state));
    }

    #[test]
    fn daily_check_skips_when_checked_recently() {
        let updater = Updater::new(ToolPaths { bin: "unused".into() }, Client::new());
        let fresh = ToolState { ytdlp_checked_at: Some(unix_now()), ..ToolState::default() };
        let stale = ToolState { ytdlp_checked_at: Some(unix_now() - DAILY.as_secs() - 1), ..ToolState::default() };
        assert!(!updater.may_ask(Channel::Stable, false, &fresh));
        assert!(updater.may_ask(Channel::Stable, false, &stale));
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
                ..Request::default()
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
        assert!(updater.update(Channel::Stable, true, None).await.unwrap());
        assert!(paths.ytdlp().is_file());
        assert!(!updater.update(Channel::Stable, false, None).await.unwrap(), "checked a moment ago");
        let version = ToolState::load(&paths.bin.join("state.json")).ytdlp;
        assert!(updater.update(Channel::Stable, true, Some("older")).await.unwrap(), "replaced since the failure");
        assert_eq!(ToolState::load(&paths.bin.join("state.json")).ytdlp, version);
    }
}
