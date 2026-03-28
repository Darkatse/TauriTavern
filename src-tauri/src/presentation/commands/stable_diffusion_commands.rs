use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::stable_diffusion_dto::SdRouteResponseDto;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn sd_handle(
    request_id: String,
    path: String,
    body: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SdRouteResponseDto, CommandError> {
    let request_id = request_id.trim().to_string();
    validate_request_id(&request_id)?;
    log_command(format!("sd_handle {} {}", request_id, path));

    app_state
        .stable_diffusion_service
        .handle_request(&request_id, path, body)
        .await
        .map_err(map_command_error("SD request failed"))
}

#[tauri::command]
pub async fn cancel_sd_request(
    request_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let request_id = request_id.trim().to_string();
    validate_request_id(&request_id)?;
    log_command(format!("cancel_sd_request {}", request_id));

    app_state
        .stable_diffusion_service
        .cancel_request(&request_id)
        .await;

    Ok(())
}

fn validate_request_id(request_id: &str) -> Result<(), CommandError> {
    let request_id = request_id.trim();
    if request_id.is_empty() || request_id.len() > 128 {
        return Err(CommandError::BadRequest(
            "Invalid request id length".to_string(),
        ));
    }

    if !request_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(CommandError::BadRequest(
            "Invalid request id characters".to_string(),
        ));
    }

    Ok(())
}
