use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::domain::models::update::UpdateCheckResult;
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn check_for_update(
    app_state: State<'_, Arc<AppState>>,
) -> Result<UpdateCheckResult, CommandError> {
    log_command("check_for_update");

    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.updates.manual_check,
        "updates.manual_check",
    )?;

    app_state
        .update_service
        .check_for_update()
        .await
        .map_err(map_command_error("Failed to check for update"))
}
