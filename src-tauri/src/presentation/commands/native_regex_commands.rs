use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::native_regex_dto::{
    NativeRegexBatchRequestDto, NativeRegexBatchResponseDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn apply_native_regex_batch(
    dto: NativeRegexBatchRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<NativeRegexBatchResponseDto, CommandError> {
    log_command("apply_native_regex_batch");

    app_state
        .native_regex_service
        .apply_batch(dto)
        .await
        .map_err(map_command_error("Failed to apply native regex batch"))
}
