use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::secret_dto::{
    AllSecretsDto, DeleteSecretDto, FindSecretDto, FindSecretResponseDto, RenameSecretDto,
    RotateSecretDto, SecretStateDto, WriteSecretDto,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn write_secret(
    dto: WriteSecretDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command(format!("write_secret {}", dto.key));

    let id = app_state
        .secret_service
        .write_secret(&dto.key, &dto.value, dto.label.as_deref())
        .await
        .map_err(map_command_error(format!(
            "Failed to write secret {}",
            dto.key
        )))?;

    Ok(id)
}

#[tauri::command]
pub async fn read_secret_state(
    app_state: State<'_, Arc<AppState>>,
) -> Result<SecretStateDto, CommandError> {
    log_command("read_secret_state");

    app_state
        .secret_service
        .read_secret_state()
        .await
        .map_err(map_command_error("Failed to read secret state"))
}

#[tauri::command]
pub async fn view_secrets(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AllSecretsDto, CommandError> {
    log_command("view_secrets");

    app_state
        .secret_service
        .view_secrets()
        .await
        .map_err(map_command_error("Failed to view secrets"))
}

#[tauri::command]
pub async fn find_secret(
    dto: FindSecretDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<FindSecretResponseDto, CommandError> {
    log_command(format!("find_secret {}", dto.key));

    app_state
        .secret_service
        .find_secret(&dto.key, dto.id.as_deref())
        .await
        .map_err(map_command_error(format!(
            "Failed to find secret {}",
            dto.key
        )))
}

#[tauri::command]
pub async fn delete_secret(
    dto: DeleteSecretDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_secret {}", dto.key));

    app_state
        .secret_service
        .delete_secret(&dto.key, dto.id.as_deref())
        .await
        .map_err(map_command_error(format!(
            "Failed to delete secret {}",
            dto.key
        )))
}

#[tauri::command]
pub async fn rotate_secret(
    dto: RotateSecretDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("rotate_secret {}", dto.key));

    app_state
        .secret_service
        .rotate_secret(&dto.key, &dto.id)
        .await
        .map_err(map_command_error(format!(
            "Failed to rotate secret {}",
            dto.key
        )))
}

#[tauri::command]
pub async fn rename_secret(
    dto: RenameSecretDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("rename_secret {}", dto.key));

    app_state
        .secret_service
        .rename_secret(&dto.key, &dto.id, &dto.label)
        .await
        .map_err(map_command_error(format!(
            "Failed to rename secret {}",
            dto.key
        )))
}
