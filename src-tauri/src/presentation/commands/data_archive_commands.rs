use tauri::{AppHandle, Manager};

use crate::infrastructure::paths::RuntimePaths;
use crate::infrastructure::persistence::data_archive_jobs::{
    DataArchiveJobStatus, UserBackupArchiveResult,
    cancel_data_archive_job as cancel_data_archive_job_impl,
    cleanup_export_data_archive as cleanup_export_data_archive_impl,
    cleanup_user_backup_archive as cleanup_user_backup_archive_impl,
    export_user_backup_archive_file as export_user_backup_archive_file_impl,
    get_data_archive_job_status as get_data_archive_job_status_impl,
    save_export_data_archive as save_export_data_archive_impl,
    save_user_backup_archive as save_user_backup_archive_impl,
    start_export_data_archive_job as start_export_data_archive_job_impl,
    start_import_data_archive_job as start_import_data_archive_job_impl,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub fn start_import_data_archive(
    app: AppHandle,
    archive_path: String,
    archive_is_temporary: bool,
) -> Result<String, CommandError> {
    log_command(format!(
        "start_import_data_archive {} temporary={}",
        archive_path, archive_is_temporary
    ));

    start_import_data_archive_job_impl(
        &app,
        std::path::Path::new(&archive_path),
        archive_is_temporary,
    )
    .map_err(map_command_error("Failed to start data archive import"))
}

#[tauri::command]
pub fn start_export_data_archive(app: AppHandle) -> Result<String, CommandError> {
    log_command("start_export_data_archive");

    start_export_data_archive_job_impl(&app)
        .map_err(map_command_error("Failed to start data archive export"))
}

#[tauri::command]
pub fn get_data_archive_imports_root(app: AppHandle) -> Result<String, CommandError> {
    log_command("get_data_archive_imports_root");

    let runtime_paths = app.state::<RuntimePaths>();
    Ok(runtime_paths
        .archive_imports_root
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
pub fn get_data_archive_job_status(job_id: String) -> Result<DataArchiveJobStatus, CommandError> {
    log_command(format!("get_data_archive_job_status {}", job_id));

    get_data_archive_job_status_impl(&job_id)
        .map_err(map_command_error("Failed to get data archive job status"))
}

#[tauri::command]
pub fn cancel_data_archive_job(job_id: String) -> Result<(), CommandError> {
    log_command(format!("cancel_data_archive_job {}", job_id));

    cancel_data_archive_job_impl(&job_id)
        .map_err(map_command_error("Failed to cancel data archive job"))
}

#[tauri::command]
pub async fn save_export_data_archive(
    app: AppHandle,
    job_id: String,
) -> Result<String, CommandError> {
    log_command(format!("save_export_data_archive {}", job_id));

    let app_handle = app.clone();
    let blocking_job_id = job_id.clone();
    let saved_path = tauri::async_runtime::spawn_blocking(move || {
        save_export_data_archive_impl(&app_handle, &blocking_job_id)
    })
    .await
    .map_err(|error| {
        CommandError::InternalServerError(format!("Save export task join error: {}", error))
    })?
    .map_err(map_command_error("Failed to save export data archive"))?;

    Ok(saved_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn cleanup_export_data_archive(job_id: String) -> Result<(), CommandError> {
    log_command(format!("cleanup_export_data_archive {}", job_id));

    cleanup_export_data_archive_impl(&job_id)
        .map_err(map_command_error("Failed to cleanup export data archive"))
}

#[tauri::command]
pub async fn export_user_backup_archive(
    app: AppHandle,
    handle: String,
    include_secrets: bool,
) -> Result<UserBackupArchiveResult, CommandError> {
    log_command(format!(
        "export_user_backup_archive {} include_secrets={}",
        handle, include_secrets
    ));

    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        export_user_backup_archive_file_impl(&app_handle, &handle, include_secrets)
    })
    .await
    .map_err(|error| {
        CommandError::InternalServerError(format!("User backup export task join error: {}", error))
    })?
    .map_err(map_command_error("Failed to export user backup archive"))
}

#[tauri::command]
pub async fn save_user_backup_archive(
    app: AppHandle,
    archive_path: String,
    file_name: String,
) -> Result<String, CommandError> {
    log_command("save_user_backup_archive");

    let app_handle = app.clone();
    let saved_path = tauri::async_runtime::spawn_blocking(move || {
        save_user_backup_archive_impl(&app_handle, &archive_path, &file_name)
    })
    .await
    .map_err(|error| {
        CommandError::InternalServerError(format!("Save user backup task join error: {}", error))
    })?
    .map_err(map_command_error("Failed to save user backup archive"))?;

    Ok(saved_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn cleanup_user_backup_archive(
    app: AppHandle,
    archive_path: String,
) -> Result<(), CommandError> {
    log_command("cleanup_user_backup_archive");

    cleanup_user_backup_archive_impl(&app, &archive_path)
        .map_err(map_command_error("Failed to cleanup user backup archive"))
}
