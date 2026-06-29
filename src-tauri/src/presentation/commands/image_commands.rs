use std::sync::Arc;

use tauri::State;

use crate::application::services::user_media_service::{
    ListUserImagesInput, UploadUserImageInput, UserImageUploadResult, UserMediaService,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn upload_user_image(
    image_base64: String,
    format: String,
    filename: Option<String>,
    ch_name: Option<String>,
    user_media: State<'_, Arc<UserMediaService>>,
) -> Result<UserImageUploadResult, CommandError> {
    log_command("upload_user_image");

    user_media
        .upload_user_image(UploadUserImageInput {
            image_base64,
            format,
            filename,
            ch_name,
        })
        .await
        .map_err(map_command_error("Failed to upload user image"))
}

#[tauri::command]
pub async fn list_user_images(
    folder: String,
    sort_field: Option<String>,
    sort_order: Option<String>,
    media_type: Option<u32>,
    user_media: State<'_, Arc<UserMediaService>>,
) -> Result<Vec<String>, CommandError> {
    log_command("list_user_images");

    user_media
        .list_user_images(ListUserImagesInput {
            folder,
            sort_field,
            sort_order,
            media_type,
        })
        .await
        .map_err(map_command_error("Failed to list user images"))
}

#[tauri::command]
pub async fn list_user_image_folders(
    user_media: State<'_, Arc<UserMediaService>>,
) -> Result<Vec<String>, CommandError> {
    log_command("list_user_image_folders");

    user_media
        .list_user_image_folders()
        .await
        .map_err(map_command_error("Failed to list user image folders"))
}

#[tauri::command]
pub async fn delete_user_image(
    path: String,
    user_media: State<'_, Arc<UserMediaService>>,
) -> Result<(), CommandError> {
    log_command("delete_user_image");

    user_media
        .delete_user_image(&path)
        .await
        .map_err(map_command_error("Failed to delete user image"))
}
