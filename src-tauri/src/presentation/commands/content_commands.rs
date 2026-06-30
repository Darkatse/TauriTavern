use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::services::content_service::ExternalImportDownloadResult;
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn initialize_default_content(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("initialize_default_content");

    app_state
        .services
        .content_service
        .initialize_default_content("default-user")
        .await
        .map_err(map_command_error("Failed to initialize default content"))
}

#[tauri::command]
pub async fn is_default_content_initialized(
    app_state: State<'_, Arc<AppState>>,
) -> Result<bool, CommandError> {
    log_command("is_default_content_initialized");

    app_state
        .services
        .content_service
        .is_default_content_initialized("default-user")
        .await
        .map_err(map_command_error(
            "Failed to check default content initialization state",
        ))
}

#[tauri::command]
pub async fn download_external_import_url(
    url: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExternalImportDownloadResult, CommandError> {
    log_command("download_external_import_url");

    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.content.external_import,
        "content.external_import",
    )?;

    app_state
        .services
        .content_service
        .download_external_import_url(&url)
        .await
        .map_err(map_command_error("Failed to download external import URL"))
}
