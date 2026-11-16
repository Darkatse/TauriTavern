use std::sync::Arc;

use serde_json::Value;
use tauri::{State, WebviewWindow};

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn translate_text(
    provider: String,
    body: Value,
    locale: String,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("translate_text {}", provider));

    let service = &app_state.services.translate_service;
    let endpoint = service
        .resolve_user_endpoint(&provider)
        .await
        .map_err(map_command_error("Translation failed"))?;
    ensure_user_endpoint_access(
        endpoint.as_ref(),
        &locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    service
        .translate(&provider, body)
        .await
        .map_err(map_command_error("Translation failed"))
}
