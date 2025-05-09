use std::sync::Arc;
use tauri::State;

use crate::application::services::user_directory_service::UserDirectoryService;
use crate::application::dto::user_directory_dto::UserDirectoryDto;
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

#[tauri::command]
pub async fn get_user_directory(
    handle: String,
    user_directory_service: State<'_, Arc<UserDirectoryService>>,
) -> Result<UserDirectoryDto, CommandError> {
    logger::debug(&format!("Command: get_user_directory {}", handle));
    
    user_directory_service.get_user_directory(&handle).await
        .map_err(|e| {
            logger::error(&format!("Failed to get user directory for {}: {}", handle, e));
            e.into()
        })
}

#[tauri::command]
pub async fn get_default_user_directory(
    user_directory_service: State<'_, Arc<UserDirectoryService>>,
) -> Result<UserDirectoryDto, CommandError> {
    logger::debug("Command: get_default_user_directory");
    
    user_directory_service.get_default_user_directory().await
        .map_err(|e| {
            logger::error(&format!("Failed to get default user directory: {}", e));
            e.into()
        })
}

#[tauri::command]
pub async fn ensure_user_directories_exist(
    handle: String,
    user_directory_service: State<'_, Arc<UserDirectoryService>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: ensure_user_directories_exist {}", handle));
    
    user_directory_service.ensure_user_directories_exist(&handle).await
        .map_err(|e| {
            logger::error(&format!("Failed to ensure directories exist for user {}: {}", handle, e));
            e.into()
        })
}

#[tauri::command]
pub async fn ensure_default_user_directories_exist(
    user_directory_service: State<'_, Arc<UserDirectoryService>>,
) -> Result<(), CommandError> {
    logger::debug("Command: ensure_default_user_directories_exist");
    
    user_directory_service.ensure_default_user_directories_exist().await
        .map_err(|e| {
            logger::error(&format!("Failed to ensure directories exist for default user: {}", e));
            e.into()
        })
}
