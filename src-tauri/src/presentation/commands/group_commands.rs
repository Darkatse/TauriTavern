use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::group_dto::{
    CreateGroupDto, DeleteGroupDto, GroupDto, UpdateGroupDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_all_groups(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<GroupDto>, CommandError> {
    log_command("get_all_groups");

    app_state
        .group_service
        .get_all_groups()
        .await
        .map(|groups| groups.into_iter().map(GroupDto::from).collect())
        .map_err(map_command_error("Failed to get all groups"))
}

#[tauri::command]
pub async fn get_group(
    id: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<GroupDto>, CommandError> {
    log_command(format!("get_group {}", id));

    app_state
        .group_service
        .get_group(&id)
        .await
        .map(|group| group.map(GroupDto::from))
        .map_err(map_command_error(format!("Failed to get group {}", id)))
}

#[tauri::command]
pub async fn create_group(
    dto: CreateGroupDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<GroupDto, CommandError> {
    log_command(format!("create_group {}", dto.name));

    app_state
        .group_service
        .create_group(dto)
        .await
        .map(GroupDto::from)
        .map_err(map_command_error("Failed to create group"))
}

#[tauri::command]
pub async fn update_group(
    dto: UpdateGroupDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<GroupDto, CommandError> {
    log_command(format!("update_group {}", dto.id));

    app_state
        .group_service
        .update_group(dto)
        .await
        .map(GroupDto::from)
        .map_err(map_command_error("Failed to update group"))
}

#[tauri::command]
pub async fn delete_group(
    dto: DeleteGroupDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_group {}", dto.id));

    app_state
        .group_service
        .delete_group(dto)
        .await
        .map_err(map_command_error("Failed to delete group"))
}

#[tauri::command]
pub async fn get_group_chat_paths(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("get_group_chat_paths");

    app_state
        .group_service
        .get_group_chat_paths()
        .await
        .map_err(map_command_error("Failed to get group chat paths"))
}

#[tauri::command]
pub async fn clear_group_cache(app_state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    log_command("clear_group_cache");

    app_state
        .group_service
        .clear_cache()
        .await
        .map_err(map_command_error("Failed to clear group cache"))
}
