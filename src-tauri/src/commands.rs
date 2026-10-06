//! The only interface the frontend can call (contract: docs/rust-rewrite/design.md).

use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

use ytf_core::reqwest;
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
