use std::sync::Arc;
use tauri::State;

use crate::application::services::user_service::UserService;
use crate::application::dto::user_dto::{UserDto, CreateUserDto, UpdateUserDto};
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

#[tauri::command]
pub async fn get_all_users(
    user_service: State<'_, Arc<UserService>>,
) -> Result<Vec<UserDto>, CommandError> {
    logger::debug("Command: get_all_users");
    
    user_service.get_all_users().await
        .map_err(|e| {
            logger::error(&format!("Failed to get all users: {}", e));
            e.into()
        })
}

#[tauri::command]
pub async fn get_user(
    id: String,
    user_service: State<'_, Arc<UserService>>,
) -> Result<UserDto, CommandError> {
    logger::debug(&format!("Command: get_user {}", id));
    
    user_service.get_user(&id).await
        .map_err(|e| {
            logger::error(&format!("Failed to get user {}: {}", id, e));
            e.into()
        })
}

#[tauri::command]
pub async fn get_user_by_username(
    username: String,
    user_service: State<'_, Arc<UserService>>,
) -> Result<UserDto, CommandError> {
    logger::debug(&format!("Command: get_user_by_username {}", username));
    
    user_service.get_user_by_username(&username).await
        .map_err(|e| {
            logger::error(&format!("Failed to get user by username {}: {}", username, e));
            e.into()
        })
}

#[tauri::command]
pub async fn create_user(
    dto: CreateUserDto,
    user_service: State<'_, Arc<UserService>>,
) -> Result<UserDto, CommandError> {
    logger::debug(&format!("Command: create_user {}", dto.username));
    
    user_service.create_user(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to create user: {}", e));
            e.into()
        })
}

#[tauri::command]
pub async fn update_user(
    dto: UpdateUserDto,
    user_service: State<'_, Arc<UserService>>,
) -> Result<UserDto, CommandError> {
    logger::debug(&format!("Command: update_user {}", dto.id));
    
    user_service.update_user(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to update user: {}", e));
            e.into()
        })
}

#[tauri::command]
pub async fn delete_user(
    id: String,
    user_service: State<'_, Arc<UserService>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_user {}", id));
    
    user_service.delete_user(&id).await
        .map_err(|e| {
            logger::error(&format!("Failed to delete user {}: {}", id, e));
            e.into()
        })
}
