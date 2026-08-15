use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::vector_dto::{VectorRouteRequestDto, VectorRouteResponseDto};

#[tauri::command]
pub async fn vector_handle(
    path: String,
    request: VectorRouteRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<VectorRouteResponseDto, CommandError> {
    log_command(format!("vector_handle {}", path.trim()));
    app_state
        .services
        .vector_service
        .handle_request(&path, request)
        .await
        .map_err(map_command_error("Vector request failed"))
}
