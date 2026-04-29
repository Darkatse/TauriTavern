use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::tts_dto::TtsRouteResponseDto;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn tts_handle(
    path: String,
    body: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<TtsRouteResponseDto, CommandError> {
    log_command(format!("tts_handle {}", path));

    app_state
        .tts_service
        .handle_request(path, body)
        .await
        .map_err(map_command_error("TTS request failed"))
}
