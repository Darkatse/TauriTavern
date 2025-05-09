use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::group_dto::{GroupDto, CreateGroupDto, UpdateGroupDto, DeleteGroupDto};
use crate::application::errors::ApplicationError;
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

/// Get all groups
#[tauri::command]
pub async fn get_all_groups(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<GroupDto>, CommandError> {
    logger::debug("Command: get_all_groups");

    app_state.group_service.get_all_groups().await
        .map(|groups| groups.into_iter().map(GroupDto::from).collect())
        .map_err(|e| {
            logger::error(&format!("Failed to get all groups: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Get a group by ID
#[tauri::command]
pub async fn get_group(
    id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<GroupDto>, CommandError> {
    logger::debug(&format!("Command: get_group {}", id));

    app_state.group_service.get_group(&id).await
        .map(|group_opt| group_opt.map(GroupDto::from))
        .map_err(|e| {
            logger::error(&format!("Failed to get group {}: {}", id, e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Create a new group
#[tauri::command]
pub async fn create_group(
    dto: CreateGroupDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<GroupDto, CommandError> {
    logger::debug(&format!("Command: create_group {}", dto.name));

    app_state.group_service.create_group(dto).await
        .map(GroupDto::from)
        .map_err(|e| {
            logger::error(&format!("Failed to create group: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Update a group
#[tauri::command]
pub async fn update_group(
    dto: UpdateGroupDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<GroupDto, CommandError> {
    logger::debug(&format!("Command: update_group {}", dto.id));

    app_state.group_service.update_group(dto).await
        .map(GroupDto::from)
        .map_err(|e| {
            logger::error(&format!("Failed to update group: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Delete a group
#[tauri::command]
pub async fn delete_group(
    dto: DeleteGroupDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_group {}", dto.id));

    app_state.group_service.delete_group(dto).await
        .map_err(|e| {
            logger::error(&format!("Failed to delete group: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Get all group chat paths
#[tauri::command]
pub async fn get_group_chat_paths(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    logger::debug("Command: get_group_chat_paths");

    app_state.group_service.get_group_chat_paths().await
        .map_err(|e| {
            logger::error(&format!("Failed to get group chat paths: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}

/// Clear the group cache
#[tauri::command]
pub async fn clear_group_cache(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug("Command: clear_group_cache");

    app_state.group_service.clear_cache().await
        .map_err(|e| {
            logger::error(&format!("Failed to clear group cache: {}", e));
            // 先将 DomainError 转换为 ApplicationError，再转换为 CommandError
            let app_error: ApplicationError = e.into();
            app_error.into()
        })
}
