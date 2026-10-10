mod app_update;
mod commands;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ytf_core::process::init();
    if let Some(dir) = ytf_core::log::default_dir() {
        ytf_core::log::init(dir);
    }
    ytf_core::applog!("starting youtube-to-your-format {}", env!("CARGO_PKG_VERSION"));
    let state = commands::AppState::new().expect("LOCALAPPDATA and the HTTP client are available");
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let busy = app.try_state::<ytf_core::queue::Queue>().is_some_and(|queue| queue.has_active());
                let confirmed = app
                    .try_state::<commands::AppState>()
                    .is_some_and(|state| state.close_confirmed.load(std::sync::atomic::Ordering::Relaxed));
                if busy && !confirmed {
                    api.prevent_close();
                    let Some(state) = app.try_state::<commands::AppState>() else { return };
                    let mut prompt = state.close_prompt.lock().unwrap();
                    let unanswered = prompt.is_some_and(|(at, shown)| !shown && at.elapsed().as_secs() < 60);
                    if unanswered {
                        // The window never showed the question: do not trap the user.
                        tauri::async_runtime::spawn(commands::shutdown_gracefully(app.clone()));
                    } else {
                        *prompt = Some((std::time::Instant::now(), false));
                        let _ = window.emit("confirm-close", ());
                    }
                }
            }
        })
        .setup(move |app| {
            let queue = commands::start_queue(app.handle(), &state);
            app.manage(state);
            app.manage(queue);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_update::install_app_update,
            commands::prepare_tools,
            commands::lookup,
            commands::enqueue,
            commands::cancel_job,
            commands::list_jobs,
            commands::diagnostics,
            commands::open_folder,
            commands::convert_files,
            commands::get_settings,
            commands::set_settings,
            commands::open_old_folder,
            commands::close_anyway,
            commands::close_prompt_shown,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
