use std::sync::Arc;

use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;
use tt_application::dto::vector_dto::{VectorRouteRequestDto, VectorRouteResponseDto};

#[tauri::command]
pub async fn vector_handle(
    path: String,
    request: VectorRouteRequestDto,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<VectorRouteResponseDto, CommandError> {
    log_command(format!("vector_handle {}", path.trim()));

    let service = &app_state.services.vector_service;
    let endpoint = service
        .resolve_user_endpoint(&path, &request)
        .map_err(map_command_error("Vector request failed"))?;
    ensure_user_endpoint_access(
        endpoint.as_ref(),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .handle_request(&path, request)
        .await
        .map_err(map_command_error("Vector request failed"))
}
