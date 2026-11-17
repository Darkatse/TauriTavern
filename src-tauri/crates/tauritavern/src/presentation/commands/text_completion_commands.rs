use std::sync::Arc;

use serde_json::Value;
use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::presentation::commands::helpers::{
    log_command, map_command_error, normalize_request_id,
};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;
use tt_application::dto::text_completion_dto::{
    TextCompletionGenerateRequestDto, TextCompletionPropsRequestDto, TextCompletionStatusDto,
    TextCompletionStatusRequestDto,
};

#[tauri::command]
pub async fn get_text_completions_status(
    dto: TextCompletionStatusRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<TextCompletionStatusDto, CommandError> {
    log_command("get_text_completions_status");

    let service = &app_state.services.text_completion_service;
    let server = service.prepare_status(dto)?;
    ensure_user_endpoint_access(
        Some(server.endpoint()),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .get_status(server)
        .await
        .map_err(map_command_error("Failed to get text completions status"))
}

#[tauri::command]
pub async fn get_text_completions_props(
    dto: TextCompletionPropsRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("get_text_completions_props");

    let service = &app_state.services.text_completion_service;
    let prepared = service.prepare_props(dto)?;
    ensure_user_endpoint_access(
        Some(prepared.endpoint()),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .get_props(prepared)
        .await
        .map_err(map_command_error("Failed to get text completions props"))
}

#[tauri::command]
pub async fn generate_text_completion(
    dto: TextCompletionGenerateRequestDto,
    request_id: String,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    let request_id = normalize_request_id(&request_id)?;
    log_command(format!("generate_text_completion {}", request_id));

    let request = app_state
        .services
        .generation_runtime
        .register_request(&request_id)?;
    let service = &app_state.services.text_completion_service;
    let prepared = service.prepare_generation(dto)?;
    ensure_user_endpoint_access(
        Some(prepared.endpoint()),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .generate_request(request, prepared)
        .await
        .map_err(map_command_error("Failed to generate text completion"))
}

#[tauri::command]
pub async fn start_text_completion_stream(
    stream_id: String,
    dto: TextCompletionGenerateRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let stream_id = normalize_request_id(&stream_id)?;
    log_command(format!("start_text_completion_stream {}", stream_id));

    let service = &app_state.services.text_completion_service;
    let prepared = service.prepare_generation(dto)?;
    ensure_user_endpoint_access(
        Some(prepared.endpoint()),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .start_stream(stream_id, prepared)
        .await
        .map_err(map_command_error("Failed to start text completion stream"))
}
