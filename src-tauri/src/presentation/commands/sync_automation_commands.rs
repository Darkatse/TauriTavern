use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::domain::models::sync_automation::{SyncAutomationConfig, SyncAutomationStatus};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn sync_automation_get_config(
    app_state: State<'_, Arc<AppState>>,
) -> Result<SyncAutomationConfig, CommandError> {
    log_command("sync_automation_get_config");

    app_state
        .services
        .sync_automation_service
        .get_config()
        .await
        .map_err(map_command_error("Failed to get sync automation config"))
}

#[tauri::command]
pub async fn sync_automation_update_config(
    app_state: State<'_, Arc<AppState>>,
    config: SyncAutomationConfig,
) -> Result<SyncAutomationConfig, CommandError> {
    log_command("sync_automation_update_config");

    app_state
        .services
        .sync_automation_service
        .update_config(config)
        .await
        .map_err(map_command_error("Failed to update sync automation config"))
}

#[tauri::command]
pub async fn sync_automation_get_status(
    app_state: State<'_, Arc<AppState>>,
) -> Result<SyncAutomationStatus, CommandError> {
    log_command("sync_automation_get_status");

    Ok(app_state
        .services
        .sync_automation_service
        .get_status()
        .await)
}
