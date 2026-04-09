use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use crate::infrastructure::paths::{
    DataRootMigration, RuntimeMode, RuntimePaths, TAURITAVERN_RUNTIME_CONFIG_VERSION,
    TauriTavernRuntimeConfig, is_effectively_empty_directory, load_runtime_config,
    runtime_config_path,
};
use crate::infrastructure::persistence::file_system::write_json_file;
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

fn runtime_mode_to_string(mode: RuntimeMode) -> String {
    match mode {
        RuntimeMode::Standard => "standard".to_string(),
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        RuntimeMode::Portable => "portable".to_string(),
    }
}

#[tauri::command]
pub fn get_runtime_paths(
    runtime_paths: State<'_, RuntimePaths>,
) -> Result<RuntimePathsDto, CommandError> {
    log_command("get_runtime_paths");

    let mut configured_data_root = None;
    let mut migration_pending = false;
    let mut migration_error = None;

    if let Some(config) = load_runtime_config(&runtime_paths.app_root).map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to load tauritavern-runtime.json: {}",
            error
        ))
    })? {
        configured_data_root = Some(config.data_root.to_string_lossy().to_string());
        migration_pending = config.migration.is_some();
        migration_error = config.migration_error;
    }

    Ok(RuntimePathsDto {
        mode: runtime_mode_to_string(runtime_paths.mode),
        data_root: runtime_paths.data_root.to_string_lossy().to_string(),
        configured_data_root,
        migration_pending,
        migration_error,
    })
}

#[tauri::command]
pub async fn set_data_root(
    data_root: String,
    runtime_paths: State<'_, RuntimePaths>,
) -> Result<(), CommandError> {
    let raw = data_root.trim();
    log_command(format!("set_data_root {}", raw));

    if raw.is_empty() {
        return Err(CommandError::BadRequest(
            "data_root is required".to_string(),
        ));
    }

    let target = PathBuf::from(raw);
    if !target.is_absolute() {
        return Err(CommandError::BadRequest(
            "data_root must be an absolute path".to_string(),
        ));
    }

    if !target.is_dir() {
        return Err(CommandError::BadRequest(format!(
            "data_root must be an existing directory: {}",
            target.display()
        )));
    }

    if !is_effectively_empty_directory(&target).map_err(|error| {
        CommandError::InternalServerError(format!("Failed to inspect data_root: {}", error))
    })? {
        return Err(CommandError::BadRequest(format!(
            "data_root must be an empty directory: {}",
            target.display()
        )));
    }

    let canonical_target = dunce::canonicalize(&target).map_err(|error| {
        CommandError::InternalServerError(format!("Failed to canonicalize path: {}", error))
    })?;
    let canonical_current = dunce::canonicalize(&runtime_paths.data_root).map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to canonicalize current data root: {}",
            error
        ))
    })?;

    if canonical_target == canonical_current {
        return Err(CommandError::BadRequest(
            "data_root is already the current data directory".to_string(),
        ));
    }

    if canonical_target.starts_with(&canonical_current) {
        return Err(CommandError::BadRequest(
            "data_root cannot be inside the current data directory".to_string(),
        ));
    }

    let config_path = runtime_config_path(&runtime_paths.app_root);
    let config = TauriTavernRuntimeConfig {
        version: TAURITAVERN_RUNTIME_CONFIG_VERSION,
        data_root: canonical_target,
        migration: Some(DataRootMigration {
            from: canonical_current,
        }),
        migration_error: None,
    };

    write_json_file(&config_path, &config)
        .await
        .map_err(map_command_error(
            "Failed to write tauritavern-runtime.json",
        ))?;

    Ok(())
}
