use std::sync::Arc;

use serde_json::Value;
use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;
use tt_application::dto::tts_dto::TtsRouteResponseDto;

#[tauri::command]
pub async fn tts_handle(
    path: String,
    body: Value,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<TtsRouteResponseDto, CommandError> {
    log_command(format!("tts_handle {}", path));

    let service = &app_state.services.tts_service;
    ensure_user_endpoint_access(
        service.resolve_user_endpoint(&path, &body).as_ref(),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .handle_request(path, body)
        .await
        .map_err(map_command_error("TTS request failed"))
}
