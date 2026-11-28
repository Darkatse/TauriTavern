use std::sync::Arc;

use serde_json::Value;
use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::app::dev_observability;
use crate::presentation::commands::helpers::{
    log_command, map_command_error, normalize_request_id,
};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;
use tt_application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use tt_application::dto::dev_observability_dto::ChatCompletionRequestPreviewDto;

#[tauri::command]
pub async fn get_chat_completions_status(
    dto: ChatCompletionStatusRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("get_chat_completions_status");

    let endpoint = app_state
        .services
        .chat_completion_service
        .resolve_status_user_endpoint(&dto)?;
    ensure_user_endpoint_access(
        endpoint.as_ref(),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    app_state
        .services
        .chat_completion_service
        .get_status(dto)
        .await
        .map_err(map_command_error("Failed to get chat completions status"))
}

#[tauri::command]
pub async fn generate_chat_completion(
    dto: ChatCompletionGenerateRequestDto,
    request_id: String,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    let request_id = normalize_request_id(&request_id)?;
    log_command(format!("generate_chat_completion {}", request_id));

    let request = app_state
        .services
        .generation_runtime
        .register_request(&request_id)?;
    let endpoint = app_state
        .services
        .chat_completion_service
        .resolve_generate_user_endpoint(&dto)?;
    ensure_user_endpoint_access(
        endpoint.as_ref(),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    app_state
        .services
        .chat_completion_service
        .generate_request(request, dto)
        .await
        .map_err(map_command_error("Failed to generate chat completion"))
}

#[tauri::command]
pub async fn start_chat_completion_stream(
    stream_id: String,
    dto: ChatCompletionGenerateRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let stream_id = normalize_request_id(&stream_id)?;
    log_command(format!("start_chat_completion_stream {}", stream_id));

    let endpoint = app_state
        .services
        .chat_completion_service
        .resolve_generate_user_endpoint(&dto)?;
    ensure_user_endpoint_access(
        endpoint.as_ref(),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    app_state
        .services
        .chat_completion_service
        .start_stream(stream_id, dto)
        .await
        .map_err(map_command_error("Failed to start chat completion stream"))
}

#[tauri::command]
pub async fn preview_chat_completion_request(
    dto: ChatCompletionGenerateRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatCompletionRequestPreviewDto, CommandError> {
    log_command("preview_chat_completion_request");

    let request = app_state
        .services
        .chat_completion_service
        .preview_request(dto)
        .await?;

    Ok(dev_observability::preview_chat_completion_request(request))
}
