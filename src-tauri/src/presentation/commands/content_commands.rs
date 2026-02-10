use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn initialize_default_content(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("initialize_default_content");

    app_state
        .content_service
        .initialize_default_content("default-user")
        .await
        .map_err(map_command_error("Failed to initialize default content"))
}

#[tauri::command]
pub async fn is_default_content_initialized(
    app_state: State<'_, Arc<AppState>>,
) -> Result<bool, CommandError> {
    log_command("is_default_content_initialized");

    app_state
        .content_service
        .is_default_content_initialized("default-user")
        .await
        .map_err(map_command_error(
            "Failed to check default content initialization state",
        ))
}
