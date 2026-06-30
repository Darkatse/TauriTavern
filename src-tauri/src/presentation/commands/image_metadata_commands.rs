use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::image_metadata_dto::{
    CreateImageMetadataFolderDto, DeleteImageMetadataFolderDto, ImageMetadataFolderAssignmentDto,
    SetImageMetadataFolderThumbnailsDto, UpdateImageMetadataFolderDto,
};
use crate::domain::models::image_metadata::{BackgroundFoldersPayload, ImageMetadataFolder};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_background_folders(
    app_state: State<'_, Arc<AppState>>,
) -> Result<BackgroundFoldersPayload, CommandError> {
    log_command("get_background_folders");

    app_state
        .services
        .image_metadata_service
        .get_background_folders()
        .await
        .map_err(map_command_error("Failed to get background folders"))
}

#[tauri::command]
pub async fn create_image_metadata_folder(
    app_state: State<'_, Arc<AppState>>,
    dto: CreateImageMetadataFolderDto,
) -> Result<ImageMetadataFolder, CommandError> {
    log_command("create_image_metadata_folder");

    app_state
        .services
        .image_metadata_service
        .create_folder(dto)
        .await
        .map_err(map_command_error("Failed to create image metadata folder"))
}

#[tauri::command]
pub async fn update_image_metadata_folder(
    app_state: State<'_, Arc<AppState>>,
    dto: UpdateImageMetadataFolderDto,
) -> Result<ImageMetadataFolder, CommandError> {
    log_command(format!("update_image_metadata_folder, id: {}", dto.id));

    app_state
        .services
        .image_metadata_service
        .update_folder(dto)
        .await
        .map_err(map_command_error("Failed to update image metadata folder"))
}

#[tauri::command]
pub async fn delete_image_metadata_folder(
    app_state: State<'_, Arc<AppState>>,
    dto: DeleteImageMetadataFolderDto,
) -> Result<(), CommandError> {
    log_command(format!("delete_image_metadata_folder, id: {}", dto.id));

    app_state
        .services
        .image_metadata_service
        .delete_folder(dto)
        .await
        .map_err(map_command_error("Failed to delete image metadata folder"))
}

#[tauri::command]
pub async fn set_image_metadata_folder_thumbnails(
    app_state: State<'_, Arc<AppState>>,
    dto: SetImageMetadataFolderThumbnailsDto,
) -> Result<(), CommandError> {
    log_command("set_image_metadata_folder_thumbnails");

    app_state
        .services
        .image_metadata_service
        .set_folder_thumbnails(dto)
        .await
        .map_err(map_command_error(
            "Failed to set image metadata folder thumbnails",
        ))
}

#[tauri::command]
pub async fn assign_images_to_metadata_folder(
    app_state: State<'_, Arc<AppState>>,
    dto: ImageMetadataFolderAssignmentDto,
) -> Result<(), CommandError> {
    log_command(format!("assign_images_to_metadata_folder, id: {}", dto.id));

    app_state
        .services
        .image_metadata_service
        .assign_images_to_folder(dto)
        .await
        .map_err(map_command_error(
            "Failed to assign images to metadata folder",
        ))
}

#[tauri::command]
pub async fn unassign_images_from_metadata_folder(
    app_state: State<'_, Arc<AppState>>,
    dto: ImageMetadataFolderAssignmentDto,
) -> Result<(), CommandError> {
    log_command(format!(
        "unassign_images_from_metadata_folder, id: {}",
        dto.id
    ));

    app_state
        .services
        .image_metadata_service
        .unassign_images_from_folder(dto)
        .await
        .map_err(map_command_error(
            "Failed to unassign images from metadata folder",
        ))
}
