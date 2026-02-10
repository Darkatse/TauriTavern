use std::path::PathBuf;

mod app;
mod application;
mod domain;
mod infrastructure;
mod presentation;

use app::{resolve_data_root, spawn_initialization};
use infrastructure::logging::logger;
use presentation::commands::registry::invoke_handler;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let log_dir = PathBuf::from("logs");
    if let Err(error) = logger::init_logger(&log_dir) {
        eprintln!("Failed to initialize logger: {}", error);
    }

    tracing::info!("Starting TauriTavern application");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let data_root = resolve_data_root(&app_handle)?;
            spawn_initialization(app_handle, data_root);
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
