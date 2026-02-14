use std::path::Path;

use tauri::AppHandle;

use crate::infrastructure::persistence::data_archive::{
    export_data_archive as export_data_archive_impl,
    import_data_archive as import_data_archive_impl, DataArchiveExportResult,
    DataArchiveImportResult,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn import_data_archive(
    app: AppHandle,
    archive_path: String,
) -> Result<DataArchiveImportResult, CommandError> {
    log_command(format!("import_data_archive {}", archive_path));

    import_data_archive_impl(&app, Path::new(&archive_path))
        .await
        .map_err(map_command_error("Failed to import data archive"))
}

#[tauri::command]
pub fn export_data_archive(app: AppHandle) -> Result<DataArchiveExportResult, CommandError> {
    log_command("export_data_archive");

    export_data_archive_impl(&app).map_err(map_command_error("Failed to export data archive"))
}
