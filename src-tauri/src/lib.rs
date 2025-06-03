use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tauri::Manager;

mod domain;
mod application;
mod infrastructure;
mod presentation;
mod app;

use app::AppState;
use infrastructure::logging::logger;

// Import all command modules
use presentation::commands::character_commands::{
    get_all_characters, get_character, create_character, create_character_with_avatar,
    update_character, delete_character, rename_character, import_character, export_character,
    update_avatar, get_character_chats_by_id, clear_character_cache
};
use presentation::commands::chat_commands::{
    get_all_chats, get_chat, get_character_chats, create_chat, add_message, rename_chat,
    delete_chat, search_chats, import_chat, export_chat, backup_chat, clear_chat_cache
};
use presentation::commands::user_commands::*;
use presentation::commands::settings_commands::*;
use presentation::commands::user_directory_commands::*;
use presentation::commands::secret_commands::*;
use presentation::commands::content_commands::*;
use presentation::commands::extension_commands::*;
use presentation::commands::avatar_commands::*;
use presentation::commands::group_commands::*;
use presentation::commands::background_commands::*;
use presentation::commands::theme_commands::*;
use presentation::commands::preset_commands::*;
use presentation::commands::bridge::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    // Initialize logger
    let log_dir = PathBuf::from("logs");
    if let Err(e) = logger::init_logger(&log_dir) {
        eprintln!("Failed to initialize logger: {}", e);
    }

    tracing::info!("Starting TauriTavern application");

    // Build Tauri application
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            // 获取 AppHandle
            let app_handle = app.handle();

            // 获取应用数据目录
            let app_data_dir = match app_handle.path().app_data_dir() {
                Ok(dir) => {
                    tracing::info!("App data directory: {:?}", dir);
                    dir
                },
                Err(e) => {
                    tracing::error!("Failed to get app data directory: {}", e);
                    return Err(e.into());
                }
            };

            // 构建数据根目录
            let data_root = app_data_dir.join("data");
            tracing::info!("Data root directory: {:?}", data_root);

            // 确保目录存在
            if let Err(e) = std::fs::create_dir_all(&data_root) {
                tracing::error!("Failed to create data root directory: {}", e);
                return Err(e.into());
            }

            // 在异步任务中初始化 AppState
            let app_handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                // 初始化应用程序状态
                match AppState::new(app_handle_clone.clone(), &data_root).await {
                    Ok(state) => {
                        // 管理应用程序状态（整个 AppState）
                        app_handle_clone.manage(Arc::new(state));

                        // 复制默认文件到用户目录
                        let app_state = app_handle_clone.state::<Arc<AppState>>();
                        let content_service = app_state.inner().content_service.clone();

                        // 初始化默认内容
                        if let Err(e) = content_service.initialize_default_content("default-user").await {
                            tracing::warn!("Failed to initialize default content: {}", e);
                        } else {
                            tracing::info!("Successfully initialized default content");
                        }

                        // 通知前端应用程序已准备就绪
                        if let Err(e) = app_handle_clone.emit("app-ready", ()) {
                            tracing::error!("Failed to emit app-ready event: {}", e);
                        } else {
                            tracing::info!("Application is ready");
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to initialize application state: {}", e);
                        // 通知前端应用程序初始化失败
                        if let Err(emit_err) = app_handle_clone.emit("app-error", e.to_string()) {
                            tracing::error!("Failed to emit app-error event: {}", emit_err);
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Character commands
            get_all_characters,
            get_character,
            create_character,
            create_character_with_avatar,
            update_character,
            delete_character,
            rename_character,
            import_character,
            export_character,
            update_avatar,
            get_character_chats_by_id,
            clear_character_cache,

            // Chat commands
            get_all_chats,
            get_chat,
            get_character_chats,
            create_chat,
            add_message,
            rename_chat,
            delete_chat,
            search_chats,
            import_chat,
            export_chat,
            backup_chat,
            clear_chat_cache,

            // User commands
            get_all_users,
            get_user,
            get_user_by_username,
            create_user,
            update_user,
            delete_user,

            // Settings commands
            get_settings,
            update_settings,
            save_user_settings,
            get_sillytavern_settings,
            create_settings_snapshot,
            get_settings_snapshots,
            load_settings_snapshot,
            restore_settings_snapshot,

            // User Directory commands
            get_user_directory,
            get_default_user_directory,
            ensure_user_directories_exist,
            ensure_default_user_directories_exist,

            // Secret commands
            write_secret,
            read_secret_state,
            view_secrets,
            find_secret,

            // Content commands
            initialize_default_content,
            is_default_content_initialized,

            // Extension commands
            get_extensions,
            install_extension,
            update_extension,
            delete_extension,
            get_extension_version,
            move_extension,

            // Avatar commands
            get_avatars,
            delete_avatar,
            upload_avatar,

            // Group commands
            get_all_groups,
            get_group,
            create_group,
            update_group,
            delete_group,
            get_group_chat_paths,
            clear_group_cache,

            // Background commands
            get_all_backgrounds,
            delete_background,
            rename_background,
            upload_background,

            // Theme commands
            save_theme,
            delete_theme,

            // Preset commands
            save_preset,
            delete_preset,
            restore_preset,
            save_openai_preset,
            delete_openai_preset,
            list_presets,
            preset_exists,
            get_preset,

            // Bridge commands
            emit_event,
            get_version,
            get_client_version,
            is_ready,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
