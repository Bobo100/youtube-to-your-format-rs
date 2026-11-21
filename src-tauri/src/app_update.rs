//! Self-update of the app from GitHub Releases (`latest.json`, signed with the updater key).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;
use ytf_core::applog;
use ytf_core::queue::Queue;

/// A slow or blocked GitHub must not keep the family waiting on the start screen.
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// The installer is ~15 MB; this only stops a stalled download, not a slow one.
/// Not set on the updater itself: its timeout covers each whole request, so a
/// short one would cut every download on a slow line.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Set while a manual install runs, so leaving and reopening Settings can't start a second one.
static MANUAL_INSTALLING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateProgress {
    version: String,
    received: u64,
    total: Option<u64>,
}

/// Installs a newer release before anything else starts. On Windows the installer
/// closes the app, so this only runs at startup while the queue is still empty.
/// Returns false when there is nothing to install or the update failed; the app
/// then carries on with the version it has.
#[tauri::command]
pub async fn install_app_update(app: AppHandle, queue: State<'_, Queue>) -> Result<bool, ()> {
    // Dev and debug builds are not installed copies; updating them would replace
    // them with the released build.
    if cfg!(debug_assertions) || queue.has_active() {
        return Ok(false);
    }
    match check_and_install(&app).await {
        Ok(installed) => Ok(installed),
        Err(err) => {
            applog!("app update failed: {err}");
            Ok(false)
        }
    }
}

/// The Settings button: installing is a separate step so the user can decline.
#[tauri::command]
pub async fn check_app_update(app: AppHandle, queue: State<'_, Queue>) -> Result<Option<String>, String> {
    if queue.has_active() {
        return Err("update_busy".to_owned());
    }
    match find_update(&app).await {
        Ok(update) => Ok(update.map(|u| u.version)),
        Err(err) => Err(failure_code(&err)),
    }
}

/// Unlike the startup path this reports failures; debug builds refuse to replace themselves.
#[tauri::command]
pub async fn install_app_update_now(app: AppHandle, queue: State<'_, Queue>) -> Result<bool, String> {
    if cfg!(debug_assertions) {
        return Err("update_unsupported".to_owned());
    }
    if queue.has_active() || MANUAL_INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("update_busy".to_owned());
    }
    let result = install_found(&app, &queue).await;
    MANUAL_INSTALLING.store(false, Ordering::SeqCst);
    result
}

async fn install_found(app: &AppHandle, queue: &Queue) -> Result<bool, String> {
    match find_update(app).await {
        // The check can take a while; a job started since then would be killed by the installer.
        Ok(Some(_)) if queue.has_active() => Err("update_busy".to_owned()),
        Ok(Some(update)) => install(app, update).await.map(|()| true).map_err(|err| failure_code(&err)),
        Ok(None) => Ok(false),
        Err(err) => Err(failure_code(&err)),
    }
}

/// Network trouble gets its own code; anything else is a plain failed update.
enum Failure {
    Network(String),
    Other(String),
}

impl Failure {
    fn detail(&self) -> &str {
        match self {
            Self::Network(detail) | Self::Other(detail) => detail,
        }
    }

    fn from_updater(err: tauri_plugin_updater::Error) -> Self {
        let detail = err.to_string();
        match err {
            tauri_plugin_updater::Error::Reqwest(_) => Self::Network(detail),
            _ => Self::Other(detail),
        }
    }
}

fn failure_code(err: &Failure) -> String {
    applog!("manual app update failed: {}", err.detail());
    match err {
        Failure::Network(_) => "network",
        Failure::Other(_) => "update_failed",
    }
    .to_owned()
}

async fn find_update(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, Failure> {
    let updater = app.updater().map_err(Failure::from_updater)?;
    tokio::time::timeout(CHECK_TIMEOUT, updater.check())
        .await
        .map_err(|_| Failure::Network("update check timed out".to_owned()))?
        .map_err(Failure::from_updater)
}

async fn check_and_install(app: &AppHandle) -> Result<bool, String> {
    let Some(update) = find_update(app).await.map_err(|f| f.detail().to_owned())? else {
        return Ok(false);
    };
    install(app, update).await.map_err(|f| f.detail().to_owned())?;
    Ok(true)
}

async fn install(app: &AppHandle, update: tauri_plugin_updater::Update) -> Result<(), Failure> {
    applog!("installing app update {} -> {}", update.current_version, update.version);
    let version = update.version.clone();
    let mut received = 0u64;
    let _ = app.emit("app-update-progress", AppUpdateProgress { version: version.clone(), received, total: None });
    let install = update.download_and_install(
        |chunk, total| {
            received += chunk as u64;
            let _ = app.emit("app-update-progress", AppUpdateProgress { version: version.clone(), received, total });
        },
        || applog!("app update downloaded, starting the installer"),
    );
    tokio::time::timeout(DOWNLOAD_TIMEOUT, install)
        .await
        .map_err(|_| Failure::Network("update download timed out".to_owned()))?
        .map_err(Failure::from_updater)
}
