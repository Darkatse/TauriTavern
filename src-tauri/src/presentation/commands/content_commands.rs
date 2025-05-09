use std::sync::Arc;
use tauri::State;
use crate::app::AppState;
use crate::infrastructure::logging::logger;

/// Initialize default content
#[tauri::command]
pub async fn initialize_default_content(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    logger::debug("Command: initialize_default_content");

    // 使用固定的 "default-user" 作为用户句柄
    app_state.content_service.initialize_default_content("default-user")
        .await
        .map_err(|e| e.to_string())
}

/// Check if default content is initialized
#[tauri::command]
pub async fn is_default_content_initialized(
    app_state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    logger::debug("Command: is_default_content_initialized");

    // 使用固定的 "default-user" 作为用户句柄
    app_state.content_service.is_default_content_initialized("default-user")
        .await
        .map_err(|e| e.to_string())
}
