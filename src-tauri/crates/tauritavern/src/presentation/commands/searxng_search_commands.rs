use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;
use tt_application::dto::searxng_search_dto::SearxngSearchRequestDto;

#[tauri::command]
pub async fn search_searxng(
    dto: SearxngSearchRequestDto,
    locale: String,
    app_handle: AppHandle,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command("search_searxng");

    let request = app_state
        .services
        .searxng_search_service
        .prepare_request(dto)
        .map_err(map_command_error("SearXNG search request is invalid"))?;
    ensure_user_endpoint_access(
        Some(request.base_url.to_string()),
        &locale,
        &app_handle,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    app_state
        .services
        .searxng_search_service
        .search(request)
        .await
        .map_err(map_command_error("SearXNG search failed"))
}
