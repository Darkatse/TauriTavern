use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::tokenization_dto::{
    DecodeTokensRequestDto, DecodeTokensResponseDto, EncodeTokensRequestDto,
    EncodeTokensResponseDto, OpenAiLogitBiasRequestDto, OpenAiLogitBiasResponseDto,
    OpenAiTokenCountBatchRequestDto, OpenAiTokenCountBatchResponseDto,
    OpenAiTokenPrefixCountRequestDto,
};

#[tauri::command]
pub async fn count_openai_tokens_batch(
    dto: OpenAiTokenCountBatchRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenAiTokenCountBatchResponseDto, CommandError> {
    log_command("count_openai_tokens_batch");

    app_state
        .services
        .tokenization_service
        .count_openai_tokens_batch(dto)
        .await
        .map_err(map_command_error("Failed to count OpenAI tokens batch"))
}

#[tauri::command]
pub async fn count_openai_token_prefixes(
    dto: OpenAiTokenPrefixCountRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenAiTokenCountBatchResponseDto, CommandError> {
    log_command("count_openai_token_prefixes");

    app_state
        .services
        .tokenization_service
        .count_openai_token_prefixes(dto)
        .await
        .map_err(map_command_error("Failed to count OpenAI token prefixes"))
}

#[tauri::command]
pub async fn encode_tokens(
    dto: EncodeTokensRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<EncodeTokensResponseDto, CommandError> {
    log_command("encode_tokens");

    app_state
        .services
        .tokenization_service
        .encode_tokens(dto)
        .await
        .map_err(map_command_error("Failed to encode tokens"))
}

#[tauri::command]
pub async fn decode_tokens(
    dto: DecodeTokensRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<DecodeTokensResponseDto, CommandError> {
    log_command("decode_tokens");

    app_state
        .services
        .tokenization_service
        .decode_tokens(dto)
        .await
        .map_err(map_command_error("Failed to decode tokens"))
}

#[tauri::command]
pub async fn build_openai_logit_bias(
    dto: OpenAiLogitBiasRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenAiLogitBiasResponseDto, CommandError> {
    log_command("build_openai_logit_bias");

    app_state
        .services
        .tokenization_service
        .build_openai_logit_bias(dto)
        .await
        .map_err(map_command_error("Failed to build OpenAI logit bias"))
}
