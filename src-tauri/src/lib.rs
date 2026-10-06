mod commands;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ytf_core::process::init();
    let state = commands::AppState::new().expect("failed to build HTTP client");
    tauri::Builder::default()
        // A second launch (double-clicking the shortcut again) focuses the open
        // window instead of starting a second app that installs into the same files.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![commands::prepare_tools])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
