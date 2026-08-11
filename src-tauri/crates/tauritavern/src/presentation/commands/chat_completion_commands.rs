use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
    ChatCompletionStreamReadResultDto,
};

#[tauri::command]
pub async fn get_chat_completions_status(
    dto: ChatCompletionStatusRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("get_chat_completions_status");

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
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    let request_id = request_id.trim().to_string();
    validate_stream_id(&request_id)?;
    log_command(format!("generate_chat_completion {}", request_id));

    app_state
        .services
        .chat_completion_service
        .generate_request(&request_id, dto)
        .await
        .map_err(map_command_error("Failed to generate chat completion"))
}

#[tauri::command]
pub async fn start_chat_completion_stream(
    stream_id: String,
    dto: ChatCompletionGenerateRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let stream_id = stream_id.trim().to_string();
    validate_stream_id(&stream_id)?;
    log_command(format!("start_chat_completion_stream {}", stream_id));

    app_state
        .services
        .chat_completion_service
        .clone()
        .start_stream(stream_id, dto)
        .await
        .map_err(map_command_error("Failed to start chat completion stream"))
}

#[tauri::command]
pub async fn read_chat_completion_stream(
    stream_id: String,
    after_seq: u64,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ChatCompletionStreamReadResultDto, CommandError> {
    let stream_id = stream_id.trim().to_string();
    validate_stream_id(&stream_id)?;

    app_state
        .services
        .chat_completion_service
        .read_stream(&stream_id, after_seq)
        .await
        .map_err(map_command_error("Failed to read chat completion stream"))
}

#[tauri::command]
pub async fn close_chat_completion_stream(
    stream_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let stream_id = stream_id.trim().to_string();
    validate_stream_id(&stream_id)?;
    log_command(format!("close_chat_completion_stream {}", stream_id));

    app_state
        .services
        .chat_completion_service
        .close_stream(&stream_id)
        .await;
    Ok(())
}

#[tauri::command]
pub async fn cancel_chat_completion_generation(
    request_id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    validate_stream_id(&request_id)?;
    log_command(format!("cancel_chat_completion_generation {}", request_id));

    app_state
        .services
        .chat_completion_service
        .cancel_generation(&request_id)
        .await;
    Ok(())
}

fn validate_stream_id(stream_id: &str) -> Result<(), CommandError> {
    let stream_id = stream_id.trim();
    if stream_id.is_empty() || stream_id.len() > 128 {
        return Err(CommandError::BadRequest(
            "Invalid stream id length".to_string(),
        ));
    }

    if !stream_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(CommandError::BadRequest(
            "Invalid stream id characters".to_string(),
        ));
    }

    Ok(())
}
