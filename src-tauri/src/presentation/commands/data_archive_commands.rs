use tauri::AppHandle;

use crate::infrastructure::persistence::data_archive_jobs::{
    DataArchiveJobStatus, cancel_data_archive_job as cancel_data_archive_job_impl,
    get_data_archive_job_status as get_data_archive_job_status_impl,
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
