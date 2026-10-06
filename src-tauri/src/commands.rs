//! The only interface the frontend can call (contract: docs/rust-rewrite/design.md).

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

use ytf_core::folders;
use ytf_core::queue::{Job, JobId, Queue, Request, SaveFormat};
use ytf_core::reqwest;
use ytf_core::runner::YtDlpRunner;
use ytf_core::tools::{self, ToolPaths};
use ytf_core::ytdlp::{self, Input, Lookup};

pub struct AppState {
    /// Serializes tool installs: React StrictMode (and impatient double clicks)
    /// would otherwise run two installs into the same files.
    tools_lock: Mutex<()>,
    client: reqwest::Client,
}

impl AppState {
    pub fn new() -> reqwest::Result<Self> {
        Ok(Self {
            tools_lock: Mutex::new(()),
            client: tools::http_client()?,
        })
    }
}

#[tauri::command]
pub async fn prepare_tools(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.tools_lock.lock().await;
    let paths = ToolPaths::from_env().map_err(|_| "tools_missing".to_owned())?;
    tools::prepare(&paths, &state.client, &|progress| {
        let _ = app.emit("tools-progress", progress);
    })
    .await
    .map_err(|err| {
        eprintln!("prepare_tools failed: {err}");
        err.code().to_owned()
    })
}

#[tauri::command]
pub async fn lookup(input: String, whole_playlist: bool) -> Result<Lookup, String> {
    let parsed = Input::parse(&input).ok_or_else(|| "empty_input".to_owned())?;
    let paths = ToolPaths::from_env().map_err(|_| "tools_missing".to_owned())?;
    let searching = matches!(parsed, Input::Search(_));
    ytdlp::lookup::lookup(&paths, &parsed, whole_playlist)
        .await
        .map_err(|err| {
            eprintln!("lookup failed: {err}");
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

#[tauri::command]
pub fn enqueue(queue: State<'_, Queue>, items: Vec<SaveItem>, format: SaveFormat) -> Vec<JobId> {
    queue.enqueue(
        items
            .into_iter()
            .map(|item| Request { video_id: item.id, url: item.url, title: item.title, format })
            .collect(),
    )
}

#[tauri::command]
pub fn cancel_job(queue: State<'_, Queue>, id: JobId) {
    queue.cancel(id);
}

/// Only reveals files this app produced, so the frontend cannot open arbitrary paths.
#[tauri::command]
pub fn open_folder(queue: State<'_, Queue>, id: JobId) -> Result<(), String> {
    let path = queue
        .jobs()
        .into_iter()
        .find(|job| job.id == id)
        .and_then(|job| job.output_path)
        .ok_or_else(|| "not_found".to_owned())?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|err| {
        eprintln!("reveal failed: {err}");
        "open_failed".to_owned()
    })
}

pub fn start_queue(app: &AppHandle) -> Queue {
    let paths = ToolPaths::from_env().expect("LOCALAPPDATA is set on Windows");
    let runner = YtDlpRunner { paths, output_dir: Box::new(folders::default_output_dir) };
    let emitter = app.clone();
    let (queue, worker) = Queue::new(Arc::new(runner), move |job: &Job| {
        let _ = emitter.emit("job-updated", job);
    });
    tauri::async_runtime::spawn(worker);
    queue
}
