use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_chat_completions_status(
    dto: ChatCompletionStatusRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("get_chat_completions_status");

    app_state
        .chat_completion_service
        .get_status(dto)
        .await
        .map_err(map_command_error("Failed to get chat completions status"))
}

#[tauri::command]
pub async fn generate_chat_completion(
    dto: ChatCompletionGenerateRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Value, CommandError> {
    log_command("generate_chat_completion");

    app_state
        .chat_completion_service
        .generate(dto)
        .await
        .map_err(map_command_error("Failed to generate chat completion"))
}
