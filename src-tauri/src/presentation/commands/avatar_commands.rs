use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::domain::models::avatar::{AvatarUploadResult, CropInfo};
use crate::infrastructure::logging::logger;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_avatars(app_state: State<'_, Arc<AppState>>) -> Result<Vec<String>, CommandError> {
    log_command("get_avatars");

    app_state
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
        .avatar_service
        .delete_avatar(&avatar)
        .await
        .map_err(map_command_error("Failed to delete avatar"))
}

#[tauri::command]
pub async fn upload_avatar(
    file_path: String,
    overwrite_name: Option<String>,
    crop: Option<String>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AvatarUploadResult, CommandError> {
    log_command(format!("upload_avatar {}", file_path));

    let crop_info = match crop {
        Some(crop_str) => match serde_json::from_str::<CropInfo>(&crop_str) {
            Ok(info) => Some(info),
            Err(error) => {
                logger::error(&format!("Failed to parse crop information: {}", error));
                None
            }
        },
        None => None,
    };

    let path = PathBuf::from(file_path);
    app_state
        .avatar_service
        .upload_avatar(&path, overwrite_name, crop_info)
        .await
        .map_err(map_command_error("Failed to upload avatar"))
}
