use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{
    log_command, map_command_error, normalize_request_id,
};
use crate::presentation::errors::CommandError;
use tt_application::dto::horde_dto::{HordeTextGenerationRequestDto, HordeUserInfoDto};

#[tauri::command]
pub async fn check_horde_status(app_state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    log_command("check_horde_status");

    app_state
        .services
        .horde_service
        .check_status()
        .await
        .map_err(map_command_error("Failed to reach the AI Horde"))
}

#[tauri::command]
pub async fn get_horde_text_workers(
    force: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("get_horde_text_workers");

    app_state
        .services
        .horde_service
        .text_workers(force)
        .await
        .map_err(map_command_error("Failed to get Horde text workers"))
}

#[tauri::command]
pub async fn get_horde_text_models(
    force: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("get_horde_text_models");

    app_state
        .services
        .horde_service
        .text_models(force)
        .await
        .map_err(map_command_error("Failed to get Horde text models"))
}

#[tauri::command]
pub async fn get_horde_user_info(
    app_state: State<'_, Arc<AppState>>,
) -> Result<HordeUserInfoDto, CommandError> {
    log_command("get_horde_user_info");

    app_state
        .services
        .horde_service
        .user_info()
        .await
        .map_err(map_command_error("Failed to get Horde user info"))
}

#[tauri::command]
pub async fn start_horde_text_stream(
    stream_id: String,
    dto: HordeTextGenerationRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let stream_id = normalize_request_id(&stream_id)?;
    log_command(format!("start_horde_text_stream {}", stream_id));

    app_state
        .services
        .horde_service
        .start_text_stream(stream_id, dto)
        .await
        .map_err(map_command_error("Failed to start Horde text generation"))
}
