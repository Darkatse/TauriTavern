use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::background_dto::{DeleteBackgroundDto, RenameBackgroundDto};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_all_backgrounds(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("get_all_backgrounds");

    app_state
        .background_service
        .get_all_backgrounds()
        .await
        .map(|backgrounds| backgrounds.into_iter().map(|bg| bg.filename).collect())
        .map_err(map_command_error("Failed to get all backgrounds"))
}

#[tauri::command]
pub async fn delete_background(
    app_state: State<'_, Arc<AppState>>,
    dto: DeleteBackgroundDto,
) -> Result<(), CommandError> {
    log_command(format!("delete_background, filename: {}", dto.bg));

    app_state
        .background_service
        .delete_background(&dto.bg)
        .await
        .map_err(map_command_error("Failed to delete background"))
}

#[tauri::command]
pub async fn rename_background(
    app_state: State<'_, Arc<AppState>>,
    dto: RenameBackgroundDto,
) -> Result<(), CommandError> {
    log_command(format!(
        "rename_background, from: {} to: {}",
        dto.old_bg, dto.new_bg
    ));

    app_state
        .background_service
        .rename_background(&dto.old_bg, &dto.new_bg)
        .await
        .map_err(map_command_error("Failed to rename background"))
}

#[tauri::command]
pub async fn upload_background(
    app_state: State<'_, Arc<AppState>>,
    filename: String,
    data: Vec<u8>,
) -> Result<String, CommandError> {
    log_command(format!("upload_background, filename: {}", filename));

    app_state
        .background_service
        .upload_background(&filename, &data)
        .await
        .map_err(map_command_error("Failed to upload background"))
}
