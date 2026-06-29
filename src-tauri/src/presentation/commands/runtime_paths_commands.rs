use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::application::services::runtime_paths_service::{
    RuntimeModeInfo, RuntimePathsInfo, RuntimePathsService,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RuntimePathsDto {
    pub mode: String,
    pub data_root: String,
    pub configured_data_root: Option<String>,
    pub migration_pending: bool,
    pub migration_error: Option<String>,
}

fn runtime_mode_to_string(mode: RuntimeModeInfo) -> String {
    match mode {
        RuntimeModeInfo::Standard => "standard".to_string(),
        RuntimeModeInfo::Portable => "portable".to_string(),
    }
}

#[tauri::command]
pub fn get_runtime_paths(
    runtime_paths: State<'_, Arc<RuntimePathsService>>,
) -> Result<RuntimePathsDto, CommandError> {
    log_command("get_runtime_paths");

    runtime_paths
        .get_runtime_paths()
        .map(runtime_paths_dto)
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn set_data_root(
    data_root: String,
    runtime_paths: State<'_, Arc<RuntimePathsService>>,
) -> Result<(), CommandError> {
    let raw = data_root.trim();
    log_command(format!("set_data_root {}", raw));

    runtime_paths
        .request_data_root_change(raw)
        .await
        .map_err(map_set_data_root_error)
}

fn runtime_paths_dto(info: RuntimePathsInfo) -> RuntimePathsDto {
    RuntimePathsDto {
        mode: runtime_mode_to_string(info.mode),
        data_root: info.data_root.to_string_lossy().to_string(),
        configured_data_root: info
            .configured_data_root
            .map(|path| path.to_string_lossy().to_string()),
        migration_pending: info.migration_pending,
        migration_error: info.migration_error,
    }
}

fn map_set_data_root_error(error: crate::domain::errors::DomainError) -> CommandError {
    let command_error = CommandError::from(error);
    if matches!(&command_error, CommandError::InternalServerError(_)) {
        map_command_error("Failed to set data root")(command_error)
    } else {
        command_error
    }
}
