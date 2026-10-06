//! The only interface the frontend can call (contract: docs/rust-rewrite/design.md).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use ytf_core::applog;
use ytf_core::folders;
use ytf_core::queue::{Job, JobId, JobKind, Queue, Request, SaveFormat};
use ytf_core::reqwest;
use ytf_core::runner::YtDlpRunner;
use ytf_core::tools::update::Updater;
use ytf_core::tools::{self, ToolPaths};
use ytf_core::ytdlp::{self, Input, Lookup};

/// The daily check runs at most every 6 hours while the app stays open for days.
const UPDATE_POLL: Duration = Duration::from_secs(6 * 60 * 60);

pub struct AppState {
    client: reqwest::Client,
    paths: ToolPaths,
    updater: Arc<Updater>,
    /// `prepare_tools` runs again after a window reload; start the poller once.
    poller_started: AtomicBool,
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let client = tools::http_client().map_err(|e| e.to_string())?;
        let paths = ToolPaths::from_env().map_err(|e| e.to_string())?;
        Ok(Self {
            updater: Updater::new(paths.clone(), client.clone()),
            client,
            paths,
            poller_started: AtomicBool::new(false),
        })
    }
}

#[tauri::command]
pub async fn prepare_tools(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        // One owner for bin\ (also serializes StrictMode's double call); the write
        // gate only when something will be installed, so a reload during a
        // download does not wait for it to finish.
        let _files = state.updater.files().await;
        let _swap = if tools::needs_install(&state.paths) { Some(state.updater.exclusive().await) } else { None };
        tools::prepare(&state.paths, &state.client, &|progress| {
            let _ = app.emit("tools-progress", progress);
        })
        .await
        .map_err(|err| {
            applog!("prepare_tools failed: {err}");
            err.code().to_owned()
        })?;
    }
    if !state.poller_started.swap(true, Ordering::Relaxed) {
        let updater = Arc::clone(&state.updater);
        let paths = state.paths.clone();
        tauri::async_runtime::spawn(async move {
            log_tool_versions(&paths).await;
            loop {
                match updater.check_daily().await {
                    Ok(true) => applog!("daily check installed a newer yt-dlp"),
                    Ok(false) => {}
                    Err(err) => applog!("daily yt-dlp check failed: {err}"),
                }
                tokio::time::sleep(UPDATE_POLL).await;
            }
        });
    }
    Ok(())
}

/// Versions and hashes of the installed tools, so a log shows exactly what ran.
async fn log_tool_versions(paths: &ToolPaths) {
    let state = std::fs::read_to_string(paths.bin.join("state.json")).unwrap_or_default();
    applog!("app {} tools {}", env!("CARGO_PKG_VERSION"), state.split_whitespace().collect::<String>());
    for exe in [paths.ytdlp(), paths.ffmpeg(), paths.ffprobe(), paths.deno()] {
        let hash = tokio::task::spawn_blocking({
            let exe = exe.clone();
            move || tools::sha256_file(&exe)
        })
        .await;
        match hash {
            Ok(Ok(hash)) => applog!("{} sha256 {hash}", exe.display()),
            _ => applog!("{} could not be hashed", exe.display()),
        }
    }
}

#[tauri::command]
pub async fn lookup(
    app: AppHandle,
    state: State<'_, AppState>,
    input: String,
    whole_playlist: bool,
) -> Result<Lookup, String> {
    let parsed = Input::parse(&input).ok_or_else(|| "empty_input".to_owned())?;
    let searching = matches!(parsed, Input::Search(_));
    let updater = &state.updater;
    let attempt = || async {
        let _running = updater.running().await;
        ytdlp::lookup::lookup(&state.paths, &parsed, whole_playlist).await
    };
    let on_repair = || {
        applog!("lookup looks broken; updating yt-dlp");
        let _ = app.emit("lookup-updating", ());
    };
    updater
        .with_repair(attempt, &on_repair, None)
        .await
        .map_err(|err| {
            applog!("lookup failed: {err}");
            err.code(searching).to_owned()
        })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveItem {
    id: String,
    url: String,
    title: String,
}

/// `skip_done`: batch saves ("全部存成音樂") leave out items already saved.
#[tauri::command]
pub fn enqueue(queue: State<'_, Queue>, items: Vec<SaveItem>, format: SaveFormat, skip_done: bool) -> Vec<JobId> {
    queue.enqueue(
        items
            .into_iter()
            .map(|item| Request { video_id: item.id, url: item.url, title: item.title, format, ..Request::default() })
            .collect(),
        skip_done,
    )
}

#[tauri::command]
pub fn cancel_job(queue: State<'_, Queue>, id: JobId) {
    queue.cancel(id);
}

/// Lets the window rebuild its state after a reload.
#[tauri::command]
pub fn list_jobs(queue: State<'_, Queue>) -> Vec<Job> {
    queue.jobs()
}

/// Text for "複製問題資訊": what failed and the recent log, for Bobo to read.
#[tauri::command]
pub fn diagnostics(queue: State<'_, Queue>, state: State<'_, AppState>, id: Option<JobId>) -> String {
    let job = id.and_then(|id| queue.jobs().into_iter().find(|job| job.id == id));
    let tools = std::fs::read_to_string(state.paths.bin.join("state.json")).unwrap_or_default();
    let mut text = format!(
        "youtube-to-your-format {} ({})\ntools: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        tools.split_whitespace().collect::<String>()
    );
    if let Some(job) = job {
        text.push_str(&format!(
            "\njob: {} [{:?}] {:?} -> {}\nurl: {}\n",
            job.title,
            job.format,
            job.state,
            job.error.as_deref().unwrap_or("-"),
            job.url
        ));
        if let Some(detail) = queue.detail(job.id) {
            text.push_str(&format!("\n{detail}\n"));
        }
    }
    text.push_str(&format!("\n--- log ---\n{}\n", ytf_core::log::tail(60)));
    text
}

/// Only reveals files this app produced, so the frontend cannot open arbitrary
/// paths. If the file was moved or deleted, opens the folder it was saved in.
#[tauri::command]
pub fn open_folder(queue: State<'_, Queue>, id: JobId) -> Result<(), String> {
    let path = queue
        .jobs()
        .into_iter()
        .find(|job| job.id == id)
        .and_then(|job| job.output_path)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "not_found".to_owned())?;
    let result = if path.exists() {
        tauri_plugin_opener::reveal_item_in_dir(&path)
    } else {
        let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_else(folders::default_output_dir);
        std::fs::create_dir_all(&dir).ok();
        tauri_plugin_opener::open_path(dir, None::<&str>)
    };
    result.map_err(|err| {
        applog!("opening the folder failed: {err}");
        "open_failed".to_owned()
    })
}

pub fn start_queue(app: &AppHandle, state: &AppState) -> Queue {
    let runner = YtDlpRunner {
        paths: state.paths.clone(),
        updater: Arc::clone(&state.updater),
        output_dir: Box::new(folders::default_output_dir),
    };
    let emitter = app.clone();
    let (queue, worker) = Queue::new(Arc::new(runner), move |job: &Job| {
        let _ = emitter.emit("job-updated", job);
    });
    tauri::async_runtime::spawn(worker);
    queue
}

/// Queues local files for conversion; `paths` come from the file picker or a drop.
#[tauri::command]
pub fn convert_files(queue: State<'_, Queue>, paths: Vec<String>, format: SaveFormat) -> Vec<JobId> {
    queue.enqueue(
        paths
            .into_iter()
            .map(|path| {
                let title = std::path::Path::new(&path)
                    .file_name()
                    .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
                Request { kind: JobKind::Convert, video_id: path.clone(), url: path, title, format }
            })
            .collect(),
        false,
    )
}
