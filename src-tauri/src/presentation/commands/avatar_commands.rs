use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use mime_guess::from_path;
use serde::Serialize;
use tauri::State;
use tokio::fs;

use crate::app::AppState;
use crate::domain::models::avatar::{AvatarUploadResult, CropInfo};
use crate::infrastructure::logging::logger;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[derive(Debug, Clone, Serialize)]
pub struct UserAvatarAssetPayload {
    pub content_base64: String,
    pub mime_type: String,
}

fn validate_user_avatar_file(value: &str) -> Result<String, CommandError> {
    let trimmed = value.trim();

    if trimmed.is_empty()
        || trimmed.contains('\0')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        return Err(CommandError::BadRequest(
            "Invalid avatar file name".to_string(),
        ));
    }

    Ok(trimmed.to_string())
}

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

#[tauri::command]
pub async fn read_user_avatar_asset(
    file: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserAvatarAssetPayload, CommandError> {
    log_command(format!("read_user_avatar_asset {}", file));

    let safe_file = validate_user_avatar_file(&file)?;
    let directories = app_state
        .user_directory_service
        .get_default_user_directory()
        .await
        .map_err(map_command_error(
            "Failed to resolve default user directories for avatar asset",
        ))?;

    let avatar_path = PathBuf::from(directories.avatars).join(&safe_file);
    let bytes = fs::read(&avatar_path)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                CommandError::NotFound(format!("Avatar not found: {}", safe_file))
            }
            _ => {
                CommandError::InternalServerError(format!("Failed to read avatar asset: {}", error))
            }
        })?;

    Ok(UserAvatarAssetPayload {
        content_base64: BASE64_STANDARD.encode(bytes),
        mime_type: from_path(&avatar_path)
            .first_or_octet_stream()
            .essence_str()
            .to_string(),
    })
}
