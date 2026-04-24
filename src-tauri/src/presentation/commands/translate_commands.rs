use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn translate_text(
    provider: String,
    body: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("translate_text {}", provider));

    app_state
        .translate_service
        .translate(&provider, body)
        .await
        .map_err(map_command_error("Translation failed"))
}
