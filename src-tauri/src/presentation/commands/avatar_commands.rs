use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::app::AppState;
use crate::application::errors::ApplicationError;
use crate::domain::models::avatar::{AvatarUploadResult, CropInfo};
use crate::infrastructure::logging::logger;
use crate::presentation::errors::CommandError;

/// Get all avatars
#[tauri::command]
pub async fn get_avatars(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    logger::debug("Command: get_avatars");

    app_state
        .avatar_service
        .get_avatars()
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to get avatars: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}

/// Delete an avatar
#[tauri::command]
pub async fn delete_avatar(
    avatar: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_avatar {}", avatar));

    app_state
        .avatar_service
        .delete_avatar(&avatar)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to delete avatar: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}

/// Upload an avatar
#[tauri::command]
pub async fn upload_avatar(
    file_path: String,
    overwrite_name: Option<String>,
    crop: Option<String>,
    app_handle: AppHandle,
    app_state: State<'_, Arc<AppState>>,
) -> Result<AvatarUploadResult, CommandError> {
    logger::debug(&format!("Command: upload_avatar {}", file_path));

    // Parse crop information if provided
    let crop_info = if let Some(crop_str) = crop {
        match serde_json::from_str::<CropInfo>(&crop_str) {
            Ok(info) => Some(info),
            Err(e) => {
                logger::error(&format!("Failed to parse crop information: {}", e));
                None
            }
        }
    } else {
        None
    };

    // Convert file path to PathBuf
    let path = PathBuf::from(file_path);

    app_state
        .avatar_service
        .upload_avatar(&path, overwrite_name, crop_info)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to upload avatar: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}
