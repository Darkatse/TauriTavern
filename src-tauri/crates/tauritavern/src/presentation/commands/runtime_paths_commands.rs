use std::sync::Arc;

use serde::Serialize;
use tauri::State;
#[cfg(desktop)]
use tauri::WebviewWindow;

#[cfg(desktop)]
use crate::platform::file_transfer;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::services::runtime_paths_service::{
    RuntimeModeInfo, RuntimePathsInfo, RuntimePathsService,
};

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

/// Saves the folder the user picks as the data root for the next start; `None` when they cancel.
#[cfg(desktop)]
#[tauri::command]
pub async fn pick_data_root(
    window: WebviewWindow,
    runtime_paths: State<'_, Arc<RuntimePathsService>>,
) -> Result<Option<String>, CommandError> {
    log_command("pick_data_root");

    let Some(directory) = file_transfer::pick_directories(&window, false)
        .await?
        .and_then(|directories| directories.into_iter().next())
    else {
        return Ok(None);
    };
    let directory = directory.to_string_lossy().into_owned();
    runtime_paths
        .request_data_root_change(&directory)
        .await
        .map_err(map_data_root_change_error)?;
    Ok(Some(directory))
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

#[cfg(desktop)]
fn map_data_root_change_error(error: tt_domain::errors::DomainError) -> CommandError {
    let command_error = CommandError::from(error);
    if matches!(&command_error, CommandError::InternalServerError(_)) {
        map_command_error("Failed to set data root")(command_error)
    } else {
        command_error
    }
}
