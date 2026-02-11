use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::tokenization_dto::{
    OpenAiDecodeRequestDto, OpenAiDecodeResponseDto, OpenAiEncodeRequestDto,
    OpenAiEncodeResponseDto, OpenAiLogitBiasRequestDto, OpenAiLogitBiasResponseDto,
    OpenAiTokenCountRequestDto, OpenAiTokenCountResponseDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn count_openai_tokens(
    dto: OpenAiTokenCountRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenAiTokenCountResponseDto, CommandError> {
    log_command("count_openai_tokens");

    app_state
        .tokenization_service
        .count_openai_tokens(dto)
        .map_err(map_command_error("Failed to count OpenAI tokens"))
}

#[tauri::command]
pub async fn encode_openai_tokens(
    dto: OpenAiEncodeRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenAiEncodeResponseDto, CommandError> {
    log_command("encode_openai_tokens");

    app_state
        .tokenization_service
        .encode_openai_tokens(dto)
        .map_err(map_command_error("Failed to encode OpenAI tokens"))
}

#[tauri::command]
pub async fn decode_openai_tokens(
    dto: OpenAiDecodeRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenAiDecodeResponseDto, CommandError> {
    log_command("decode_openai_tokens");

    app_state
        .tokenization_service
        .decode_openai_tokens(dto)
        .map_err(map_command_error("Failed to decode OpenAI tokens"))
}

#[tauri::command]
pub async fn build_openai_logit_bias(
    dto: OpenAiLogitBiasRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenAiLogitBiasResponseDto, CommandError> {
    log_command("build_openai_logit_bias");

    app_state
        .tokenization_service
        .build_openai_logit_bias(dto)
        .map_err(map_command_error("Failed to build OpenAI logit bias"))
}
