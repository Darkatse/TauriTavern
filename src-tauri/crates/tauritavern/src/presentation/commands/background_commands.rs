use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::background_dto::{DeleteBackgroundDto, RenameBackgroundDto};
use tt_domain::models::background::BackgroundListEntry;
use tt_domain::models::image_metadata::ImageMetadataIndex;

#[tauri::command]
pub async fn get_all_backgrounds(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<BackgroundListEntry>, CommandError> {
    log_command("get_all_backgrounds");

    app_state
        .services
        .image_metadata_service
        .get_background_list_entries()
        .await
        .map_err(map_command_error("Failed to get all backgrounds"))
}

#[tauri::command]
pub async fn get_all_background_metadata(
    app_state: State<'_, Arc<AppState>>,
    prefix: Option<String>,
) -> Result<ImageMetadataIndex, CommandError> {
    log_command(format!(
        "get_all_background_metadata, prefix: {}",
        prefix.clone().unwrap_or_default()
    ));

    app_state
        .services
        .image_metadata_service
        .get_all_background_metadata(prefix.as_deref())
        .await
        .map_err(map_command_error("Failed to get background metadata"))
}

#[tauri::command]
pub async fn delete_background(
    app_state: State<'_, Arc<AppState>>,
    dto: DeleteBackgroundDto,
) -> Result<(), CommandError> {
    log_command(format!("delete_background, filename: {}", dto.bg));

    app_state
        .services
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
        .services
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
        .services
        .background_service
        .upload_background(&filename, &data)
        .await
        .map_err(map_command_error("Failed to upload background"))
}

#[tauri::command]
pub async fn upload_background_from_path(
    app_state: State<'_, Arc<AppState>>,
    filename: String,
    file_path: String,
) -> Result<String, CommandError> {
    log_command(format!(
        "upload_background_from_path, filename: {}",
        filename
    ));

    app_state
        .services
        .background_service
        .upload_background_from_path(&filename, std::path::Path::new(&file_path))
        .await
        .map_err(map_command_error("Failed to upload background from path"))
}
