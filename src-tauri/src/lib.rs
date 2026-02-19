mod app;
mod application;
mod domain;
mod infrastructure;
mod presentation;

use app::{resolve_data_root, resolve_log_root, spawn_initialization};
use infrastructure::logging::logger;
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
        .setup(move |app| {
            let app_handle = app.handle().clone();
            logger::bind_app_handle(app_handle.clone());

            match resolve_log_root(&app_handle) {
                Ok(log_root) => {
                    if let Err(error) = logger::init_logger(&log_root) {
                        eprintln!("Failed to initialize logger: {}", error);
                    }
                }
                Err(error) => {
                    eprintln!("Failed to resolve log directory: {}", error);
                }
            }

            tracing::info!("Starting TauriTavern application");

            let data_root = resolve_data_root(&app_handle)?;
            if let Err(error) = app_handle
                .asset_protocol_scope()
                .allow_directory(&data_root, true)
            {
                tracing::warn!(
                    "Failed to extend asset protocol scope for {:?}: {}",
                    data_root,
                    error
                );
            }
            spawn_initialization(app_handle, data_root);
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
