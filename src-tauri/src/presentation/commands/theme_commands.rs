use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::theme_dto::{SaveThemeDto, DeleteThemeDto, ThemeDto};
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

/// Save a theme
#[tauri::command]
pub async fn save_theme(
    dto: SaveThemeDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: save_theme, name: {}", dto.name));
    
    // Extract name and data from the DTO
    let name = dto.name;
    let data = dto.data;
    
    // Save the theme
    app_state.theme_service.save_theme(&name, data).await
        .map_err(|e| {
            logger::error(&format!("Failed to save theme {}: {}", name, e));
            e.into()
        })
}

/// Delete a theme
#[tauri::command]
pub async fn delete_theme(
    dto: DeleteThemeDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_theme, name: {}", dto.name));
    
    // Delete the theme
    app_state.theme_service.delete_theme(&dto.name).await
        .map_err(|e| {
            logger::error(&format!("Failed to delete theme {}: {}", dto.name, e));
            e.into()
        })
}
