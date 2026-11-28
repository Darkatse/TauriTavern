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
    TextCompletionEncodeRequestDto, TextCompletionGenerateRequestDto,
    TextCompletionOllamaDownloadRequestDto, TextCompletionPropsRequestDto, TextCompletionStatusDto,
    TextCompletionStatusRequestDto, TextCompletionTabbyDownloadRequestDto, TextCompletionTokensDto,
};

async fn authorize_endpoint(
    endpoint: Option<&url::Url>,
    locale: &str,
    window: &WebviewWindow,
    app_state: &AppState,
) -> Result<(), CommandError> {
    ensure_user_endpoint_access(
        endpoint,
        locale,
        window,
        &app_state.services.user_endpoint_access_service,
    )
    .await
}

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
    authorize_endpoint(server.user_endpoint(), &locale, &window, &app_state).await?;

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
    authorize_endpoint(prepared.user_endpoint(), &locale, &window, &app_state).await?;

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
    authorize_endpoint(prepared.user_endpoint(), &locale, &window, &app_state).await?;

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
    authorize_endpoint(prepared.user_endpoint(), &locale, &window, &app_state).await?;

    service
        .start_stream(stream_id, prepared)
        .await
        .map_err(map_command_error("Failed to start text completion stream"))
}

#[tauri::command]
pub async fn encode_text_completion_tokens(
    dto: TextCompletionEncodeRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<TextCompletionTokensDto, CommandError> {
    log_command("encode_text_completion_tokens");

    let service = &app_state.services.text_completion_service;
    let prepared = service.prepare_encode(dto)?;
    authorize_endpoint(prepared.user_endpoint(), &locale, &window, &app_state).await?;

    service.encode(prepared).await.map_err(map_command_error(
        "Failed to tokenize with the text completion server",
    ))
}

#[tauri::command]
pub async fn download_ollama_model(
    dto: TextCompletionOllamaDownloadRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("download_ollama_model");

    let service = &app_state.services.text_completion_service;
    let prepared = service.prepare_ollama_download(dto)?;
    authorize_endpoint(prepared.user_endpoint(), &locale, &window, &app_state).await?;

    service
        .download(prepared)
        .await
        .map_err(map_command_error("Failed to download the Ollama model"))
}

#[tauri::command]
pub async fn download_tabby_model(
    dto: TextCompletionTabbyDownloadRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("download_tabby_model");

    let service = &app_state.services.text_completion_service;
    let prepared = service.prepare_tabby_download(dto)?;
    authorize_endpoint(prepared.user_endpoint(), &locale, &window, &app_state).await?;

    service
        .download(prepared)
        .await
        .map_err(map_command_error("Failed to download the TabbyAPI model"))
}
