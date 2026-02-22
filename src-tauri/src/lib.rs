mod app;
mod application;
mod domain;
mod infrastructure;
mod presentation;

use app::spawn_initialization;
use infrastructure::logging::logger;
use infrastructure::paths::resolve_runtime_paths;
use presentation::commands::registry::invoke_handler;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            logger::bind_app_handle(app_handle.clone());

            let runtime_paths = resolve_runtime_paths(&app_handle)?;

            if let Err(error) = logger::init_logger(&runtime_paths.log_root) {
                eprintln!("Failed to initialize logger: {}", error);
            }

            tracing::debug!("Starting TauriTavern application");

            if let Err(error) = app_handle
                .asset_protocol_scope()
                .allow_directory(&runtime_paths.data_root, true)
            {
                tracing::warn!(
                    "Failed to extend asset protocol scope for {:?}: {}",
                    runtime_paths.data_root,
                    error
                );
            }
            spawn_initialization(app_handle, runtime_paths);
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
