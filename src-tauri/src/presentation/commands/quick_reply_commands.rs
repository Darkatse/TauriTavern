use serde_json::Value;
use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn save_quick_reply_set(
    payload: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_quick_reply_set");

    app_state
        .quick_reply_service
        .save_quick_reply_set(payload)
        .await
        .map_err(map_command_error("Failed to save quick reply set"))
}

#[tauri::command]
pub async fn delete_quick_reply_set(
    payload: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("delete_quick_reply_set");

    app_state
        .quick_reply_service
        .delete_quick_reply_set(payload)
        .await
        .map_err(map_command_error("Failed to delete quick reply set"))
}
