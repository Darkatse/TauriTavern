use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::llm_connection_dto::{
    ListLlmConnectionsResultDto, LlmConnectionIdDto, LoadLlmConnectionResultDto,
    SaveLlmConnectionDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn list_llm_connections(
    app_state: State<'_, Arc<AppState>>,
) -> Result<ListLlmConnectionsResultDto, CommandError> {
    log_command("list_llm_connections");

    app_state
        .services
        .llm_connection_service
        .list_connections()
        .await
        .map(|connections| ListLlmConnectionsResultDto { connections })
        .map_err(map_command_error("Failed to list LLM connections"))
}

#[tauri::command]
pub async fn load_llm_connection(
    dto: LlmConnectionIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<LoadLlmConnectionResultDto, CommandError> {
    log_command("load_llm_connection");

    app_state
        .services
        .llm_connection_service
        .load_connection(&dto.connection_id)
        .await
        .map(|connection| LoadLlmConnectionResultDto { connection })
        .map_err(map_command_error("Failed to load LLM connection"))
}

#[tauri::command]
pub async fn save_llm_connection(
    dto: SaveLlmConnectionDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_llm_connection");

    app_state
        .services
        .llm_connection_service
        .save_connection(dto.connection)
        .await
        .map_err(map_command_error("Failed to save LLM connection"))
}

#[tauri::command]
pub async fn delete_llm_connection(
    dto: LlmConnectionIdDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("delete_llm_connection");

    app_state
        .services
        .llm_connection_service
        .delete_connection(&dto.connection_id)
        .await
        .map_err(map_command_error("Failed to delete LLM connection"))
}
