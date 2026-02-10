use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::user_dto::{CreateUserDto, UpdateUserDto, UserDto};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_all_users(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<UserDto>, CommandError> {
    log_command("get_all_users");

    app_state
        .user_service
        .get_all_users()
        .await
        .map_err(map_command_error("Failed to get all users"))
}

#[tauri::command]
pub async fn get_user(
    id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserDto, CommandError> {
    log_command(format!("get_user {}", id));

    app_state
        .user_service
        .get_user(&id)
        .await
        .map_err(map_command_error(format!("Failed to get user {}", id)))
}

#[tauri::command]
pub async fn get_user_by_username(
    username: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserDto, CommandError> {
    log_command(format!("get_user_by_username {}", username));

    app_state
        .user_service
        .get_user_by_username(&username)
        .await
        .map_err(map_command_error(format!(
            "Failed to get user by username {}",
            username
        )))
}

#[tauri::command]
pub async fn create_user(
    dto: CreateUserDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserDto, CommandError> {
    log_command(format!("create_user {}", dto.username));

    app_state
        .user_service
        .create_user(dto)
        .await
        .map_err(map_command_error("Failed to create user"))
}

#[tauri::command]
pub async fn update_user(
    dto: UpdateUserDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<UserDto, CommandError> {
    log_command(format!("update_user {}", dto.id));

    app_state
        .user_service
        .update_user(dto)
        .await
        .map_err(map_command_error("Failed to update user"))
}

#[tauri::command]
pub async fn delete_user(
    id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_user {}", id));

    app_state
        .user_service
        .delete_user(&id)
        .await
        .map_err(map_command_error(format!("Failed to delete user {}", id)))
}
