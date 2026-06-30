use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::data_archive_dto::{DataArchiveJobStatus, UserBackupArchiveResult};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub fn start_import_data_archive(
    app_state: State<'_, Arc<AppState>>,
    archive_path: String,
    archive_is_temporary: bool,
) -> Result<String, CommandError> {
    log_command(format!(
        "start_import_data_archive {} temporary={}",
        archive_path, archive_is_temporary
    ));

    app_state
        .services
        .data_archive_service
        .start_import(std::path::Path::new(&archive_path), archive_is_temporary)
        .map_err(map_command_error("Failed to start data archive import"))
}

#[tauri::command]
pub fn start_export_data_archive(
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command("start_export_data_archive");

    app_state
        .services
        .data_archive_service
        .start_export()
        .map_err(map_command_error("Failed to start data archive export"))
}

#[tauri::command]
pub fn prepare_data_archive_import_target_path(
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command("prepare_data_archive_import_target_path");

    let path = app_state
        .services
        .data_archive_service
        .prepare_incoming_import_archive_path()
        .map_err(map_command_error(
            "Failed to prepare data archive import target path",
        ))?;

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_data_archive_job_status(
    app_state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<DataArchiveJobStatus, CommandError> {
    log_command(format!("get_data_archive_job_status {}", job_id));

    app_state
        .services
        .data_archive_service
        .get_status(&job_id)
        .map_err(map_command_error("Failed to get data archive job status"))
}

#[tauri::command]
pub fn cancel_data_archive_job(
    app_state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), CommandError> {
    log_command(format!("cancel_data_archive_job {}", job_id));

    app_state
        .services
        .data_archive_service
        .cancel(&job_id)
        .map_err(map_command_error("Failed to cancel data archive job"))
}

#[tauri::command]
pub async fn save_export_data_archive(
    app_state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<String, CommandError> {
    log_command(format!("save_export_data_archive {}", job_id));

    let saved_path = app_state
        .services
        .data_archive_service
        .save_export(job_id)
        .await
        .map_err(map_command_error("Failed to save export data archive"))?;

    Ok(saved_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn cleanup_export_data_archive(
    app_state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), CommandError> {
    log_command(format!("cleanup_export_data_archive {}", job_id));

    app_state
        .services
        .data_archive_service
        .cleanup_export(&job_id)
        .map_err(map_command_error("Failed to cleanup export data archive"))
}

#[tauri::command]
pub fn finalize_export_data_archive_delivery(
    app_state: State<'_, Arc<AppState>>,
    job_id: String,
    saved_path: Option<String>,
) -> Result<Option<String>, CommandError> {
    log_command(format!("finalize_export_data_archive_delivery {}", job_id));

    let saved_target = saved_path
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty());

    app_state
        .services
        .data_archive_service
        .finalize_export_delivery(&job_id, saved_target)
        .map_err(map_command_error(
            "Failed to finalize export data archive delivery",
        ))
}

#[tauri::command]
pub async fn export_user_backup_archive(
    app_state: State<'_, Arc<AppState>>,
    handle: String,
    include_secrets: bool,
) -> Result<UserBackupArchiveResult, CommandError> {
    log_command(format!(
        "export_user_backup_archive {} include_secrets={}",
        handle, include_secrets
    ));

    app_state
        .services
        .data_archive_service
        .export_user_backup(handle, include_secrets)
        .await
        .map_err(map_command_error("Failed to export user backup archive"))
}

#[tauri::command]
pub async fn save_user_backup_archive(
    app_state: State<'_, Arc<AppState>>,
    archive_path: String,
    file_name: String,
) -> Result<String, CommandError> {
    log_command("save_user_backup_archive");

    let saved_path = app_state
        .services
        .data_archive_service
        .save_user_backup(archive_path, file_name)
        .await
        .map_err(map_command_error("Failed to save user backup archive"))?;

    Ok(saved_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn cleanup_user_backup_archive(
    app_state: State<'_, Arc<AppState>>,
    archive_path: String,
) -> Result<(), CommandError> {
    log_command("cleanup_user_backup_archive");

    app_state
        .services
        .data_archive_service
        .cleanup_user_backup(&archive_path)
        .map_err(map_command_error("Failed to cleanup user backup archive"))
}
