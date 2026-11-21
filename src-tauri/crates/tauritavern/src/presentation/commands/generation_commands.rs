use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{
    log_command, map_command_error, normalize_request_id,
};
use crate::presentation::errors::CommandError;
use tt_application::dto::generation_stream_dto::GenerationStreamReadResultDto;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationNotificationStateDto {
    pub enabled: bool,
    pub background_shows_progress_ui: bool,
}

#[tauri::command]
pub fn get_generation_notification_state(
    app_state: State<'_, Arc<AppState>>,
) -> GenerationNotificationStateDto {
    log_command("get_generation_notification_state");

    let runtime = &app_state.services.generation_runtime;
    GenerationNotificationStateDto {
        enabled: runtime.notifications_enabled(),
        background_shows_progress_ui: runtime.background_shows_progress_ui(),
    }
}

#[tauri::command]
pub async fn read_generation_stream(
    stream_id: String,
    after_seq: u64,
    app_state: State<'_, Arc<AppState>>,
) -> Result<GenerationStreamReadResultDto, CommandError> {
    let stream_id = normalize_request_id(&stream_id)?;

    app_state
        .services
        .generation_runtime
        .read_stream(&stream_id, after_seq)
        .await
        .map_err(map_command_error("Failed to read generation stream"))
}

#[tauri::command]
pub async fn close_generation_stream(
    stream_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let stream_id = normalize_request_id(&stream_id)?;
    log_command(format!("close_generation_stream {}", stream_id));

    app_state
        .services
        .generation_runtime
        .close_stream(&stream_id)
        .await;
    Ok(())
}

#[tauri::command]
pub fn cancel_generation(
    request_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let request_id = normalize_request_id(&request_id)?;
    log_command(format!("cancel_generation {}", request_id));

    app_state
        .services
        .generation_runtime
        .cancel_request(&request_id);
    Ok(())
}
