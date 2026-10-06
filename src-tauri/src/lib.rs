mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = commands::AppState::new().expect("failed to build HTTP client");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![commands::prepare_tools])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
