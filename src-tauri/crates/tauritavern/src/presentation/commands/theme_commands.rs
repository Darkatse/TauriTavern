use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::theme_dto::DeleteThemeDto;

#[tauri::command]
pub async fn save_theme(
    json: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("save_theme");

    app_state
        .services
        .theme_service
        .save_theme(&json)
        .await
        .map_err(map_command_error("Failed to save theme"))
}

#[tauri::command]
pub async fn delete_theme(
    dto: DeleteThemeDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_theme, name: {}", dto.name));

    app_state
        .services
        .theme_service
        .delete_theme(&dto.name)
        .await
        .map_err(map_command_error(format!(
            "Failed to delete theme {}",
            dto.name
        )))
}
