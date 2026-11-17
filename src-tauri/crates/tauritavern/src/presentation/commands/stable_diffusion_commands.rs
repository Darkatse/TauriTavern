use std::sync::Arc;

use serde_json::Value;
use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error, normalize_request_id,
};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;
use tt_application::dto::stable_diffusion_dto::SdRouteResponseDto;

#[tauri::command]
pub async fn sd_handle(
    request_id: String,
    path: String,
    body: Value,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SdRouteResponseDto, CommandError> {
    let request_id = normalize_request_id(&request_id)?;
    log_command(format!("sd_handle {} {}", request_id, path));

    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.ai.image_generation,
        "ai.image_generation",
    )?;

    let service = &app_state.services.stable_diffusion_service;
    let endpoint = service
        .resolve_user_endpoint(&path, &body)
        .map_err(map_command_error("SD request failed"))?;
    ensure_user_endpoint_access(
        endpoint.as_ref(),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .handle_request(&request_id, path, body)
        .await
        .map_err(map_command_error("SD request failed"))
}

#[tauri::command]
pub fn cancel_sd_request(
    request_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let request_id = normalize_request_id(&request_id)?;
    log_command(format!("cancel_sd_request {}", request_id));

    app_state
        .services
        .stable_diffusion_service
        .cancel_request(&request_id);

    Ok(())
}
