use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app::AppState;
use crate::infrastructure::staging::validate_staged_path;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::avatar_image_dto::ImageCropDto;
use tt_domain::models::avatar::AvatarUploadResult;

#[tauri::command]
pub async fn get_personas(
    app_state: State<'_, Arc<AppState>>,
) -> Result<tt_domain::models::persona::Personas, CommandError> {
    app_state
        .services
        .avatar_service
        .get_personas()
        .await
        .map_err(map_command_error("Failed to get personas"))
}

#[tauri::command]
pub async fn get_avatars(app_state: State<'_, Arc<AppState>>) -> Result<Vec<String>, CommandError> {
    log_command("get_avatars");

    app_state
        .services
        .avatar_service
        .get_avatars()
        .await
        .map_err(map_command_error("Failed to get avatars"))
}

#[tauri::command]
pub async fn delete_avatar(
    avatar: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_avatar {}", avatar));

    app_state
        .services
        .avatar_service
        .delete_avatar(&avatar)
        .await
        .map_err(map_command_error("Failed to delete avatar"))
}

#[tauri::command]
pub async fn upload_avatar(
    app: AppHandle,
    file_path: String,
    overwrite_name: Option<String>,
    crop: Option<ImageCropDto>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AvatarUploadResult, CommandError> {
    log_command(format!("upload_avatar {}", file_path));

    let path = validate_staged_path(&app, &file_path).await?;
    app_state
        .services
        .avatar_service
        .upload_avatar(&path, overwrite_name, crop)
        .await
        .map_err(map_command_error("Failed to upload avatar"))
}
