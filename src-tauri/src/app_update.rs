//! Self-update of the app from GitHub Releases (`latest.json`, signed with the updater key).

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

async fn check_and_install(app: &AppHandle) -> Result<bool, String> {
    let updater = app.updater().map_err(|err| err.to_string())?;
    let checked = tokio::time::timeout(CHECK_TIMEOUT, updater.check())
        .await
        .map_err(|_| "update check timed out".to_owned())?
        .map_err(|err| err.to_string())?;
    let Some(update) = checked else {
        return Ok(false);
    };
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
        .map_err(|_| "update download timed out".to_owned())?
        .map_err(|err| err.to_string())?;
    Ok(true)
}
