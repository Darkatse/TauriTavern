use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::theme_dto::{DeleteThemeDto, SaveThemeDto};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn save_theme(
    dto: SaveThemeDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let theme_name = dto.name;
    log_command(format!("save_theme, name: {}", theme_name));

    app_state
        .theme_service
        .save_theme(&theme_name, dto.data)
        .await
        .map_err(map_command_error(format!(
            "Failed to save theme {}",
            theme_name
        )))
}

#[tauri::command]
pub async fn delete_theme(
    dto: DeleteThemeDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_theme, name: {}", dto.name));

    app_state
        .theme_service
        .delete_theme(&dto.name)
        .await
        .map_err(map_command_error(format!(
            "Failed to delete theme {}",
            dto.name
        )))
}
