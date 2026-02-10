use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::user_directory_dto::UserDirectoryDto;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_user_directory(
    handle: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserDirectoryDto, CommandError> {
    log_command(format!("get_user_directory {}", handle));

    app_state
        .user_directory_service
        .get_user_directory(&handle)
        .await
        .map_err(map_command_error(format!(
            "Failed to get user directory for {}",
            handle
        )))
}

#[tauri::command]
pub async fn get_default_user_directory(
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserDirectoryDto, CommandError> {
    log_command("get_default_user_directory");

    app_state
        .user_directory_service
        .get_default_user_directory()
        .await
        .map_err(map_command_error("Failed to get default user directory"))
}

#[tauri::command]
pub async fn ensure_user_directories_exist(
    handle: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("ensure_user_directories_exist {}", handle));

    app_state
        .user_directory_service
        .ensure_user_directories_exist(&handle)
        .await
        .map_err(map_command_error(format!(
            "Failed to ensure directories exist for user {}",
            handle
        )))
}

#[tauri::command]
pub async fn ensure_default_user_directories_exist(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("ensure_default_user_directories_exist");

    app_state
        .user_directory_service
        .ensure_default_user_directories_exist()
        .await
        .map_err(map_command_error(
            "Failed to ensure directories exist for default user",
        ))
}
