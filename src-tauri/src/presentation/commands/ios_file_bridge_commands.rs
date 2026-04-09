#![cfg(target_os = "ios")]

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::domain::errors::DomainError;
use crate::infrastructure::ios_document_picker::{
    PickZipResult, copy_picked_url_to_path, pick_zip_archive,
};
use crate::infrastructure::ios_share_sheet::share_file;
use crate::infrastructure::paths::{IOS_EXPORT_STAGING_ROOT_NAME, resolve_runtime_paths};
use crate::infrastructure::persistence::data_archive_jobs::{
    cleanup_export_data_archive as cleanup_export_data_archive_impl,
    get_data_archive_job_status as get_data_archive_job_status_impl,
    start_import_data_archive_job as start_import_data_archive_job_impl,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[derive(Debug, Clone, Serialize)]
pub struct IosImportArchiveResponse {
    pub cancelled: bool,
    pub job_id: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IosShareExportArchiveResponse {
    pub completed: bool,
    pub activity: Option<String>,
    pub cleanup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IosShareFileResponse {
    pub completed: bool,
    pub activity: Option<String>,
}

#[tauri::command]
pub async fn ios_import_data_archive_from_picker(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<IosImportArchiveResponse, CommandError> {
    log_command("ios_import_data_archive_from_picker");

    let picked = match pick_zip_archive(&window)
        .await
        .map_err(map_command_error("Failed to present iOS document picker"))?
    {
        PickZipResult::Cancelled => {
            return Ok(IosImportArchiveResponse {
                cancelled: true,
                job_id: None,
                file_name: None,
            });
        }
        PickZipResult::Picked(picked) => picked,
    };

    let app_handle = app.clone();
    let picked_url = picked.url.clone();
    let picked_file_name = picked.file_name.clone();

    let job_id = tauri::async_runtime::spawn_blocking(move || -> Result<String, DomainError> {
        let target_path = prepare_incoming_import_archive_path(&app_handle)?;
        let _cleanup_target = CleanupTempFile::new(target_path.clone());

        copy_picked_url_to_path(&picked_url, &target_path)?;
        start_import_data_archive_job_impl(&app_handle, &target_path, true)
    })
    .await
    .map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to join iOS import staging task: {}",
            error
        ))
    })?
    .map_err(map_command_error("Failed to start data archive import"))?;

    Ok(IosImportArchiveResponse {
        cancelled: false,
        job_id: Some(job_id),
        file_name: Some(picked_file_name),
    })
}

struct CleanupTempFile {
    path: PathBuf,
}

impl CleanupTempFile {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for CleanupTempFile {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to cleanup temporary file {}: {}",
                    self.path.display(),
                    error
                );
            }
        }
    }
}

fn prepare_incoming_import_archive_path(app: &AppHandle) -> Result<PathBuf, DomainError> {
    let runtime_paths = resolve_runtime_paths(app).map_err(|error| {
        DomainError::InternalError(format!("Failed to resolve runtime paths: {}", error))
    })?;

    let incoming_dir = runtime_paths.archive_imports_root.join("incoming");
    fs::create_dir_all(&incoming_dir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create import staging directory {}: {}",
            incoming_dir.display(),
            error
        ))
    })?;

    Ok(incoming_dir.join(format!(
        "tauritavern-import-{}.zip",
        uuid::Uuid::new_v4().simple()
    )))
}

async fn present_ios_share_sheet_for_path(
    window: &WebviewWindow,
    file_path: &std::path::Path,
) -> Result<IosShareFileResponse, CommandError> {
    let share_result = share_file(window, file_path)
        .await
        .map_err(map_command_error("Failed to present iOS share sheet"))?;

    Ok(IosShareFileResponse {
        completed: share_result.completed,
        activity: share_result.activity,
    })
}

fn resolve_allowed_ios_share_roots(window: &WebviewWindow) -> Result<Vec<PathBuf>, CommandError> {
    let path_resolver = window.app_handle().path();
    let candidate_roots = [
        path_resolver
            .app_cache_dir()
            .map(|path| path.join(IOS_EXPORT_STAGING_ROOT_NAME))
            .map_err(|error| {
                CommandError::InternalServerError(format!(
                    "Failed to resolve iOS app cache directory: {}",
                    error
                ))
            })?,
        path_resolver
            .temp_dir()
            .map(|path| path.join(IOS_EXPORT_STAGING_ROOT_NAME))
            .map_err(|error| {
                CommandError::InternalServerError(format!(
                    "Failed to resolve iOS temp directory: {}",
                    error
                ))
            })?,
    ];

    let mut allowed_roots = Vec::new();
    for root in candidate_roots {
        match fs::canonicalize(&root) {
            Ok(path) => allowed_roots.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CommandError::InternalServerError(format!(
                    "Failed to resolve iOS share staging root {}: {}",
                    root.display(),
                    error
                )));
            }
        }
    }

    if allowed_roots.is_empty() {
        return Err(CommandError::InternalServerError(
            "No iOS share staging root is available".to_string(),
        ));
    }

    Ok(allowed_roots)
}

fn resolve_ios_shareable_file_path(
    window: &WebviewWindow,
    file_path: &str,
) -> Result<PathBuf, CommandError> {
    let requested_path = PathBuf::from(file_path);
    if !requested_path.is_absolute() {
        return Err(CommandError::BadRequest(
            "iOS share path must be absolute".to_string(),
        ));
    }

    let canonical_file_path = fs::canonicalize(&requested_path)
        .map_err(|_| CommandError::BadRequest("Invalid iOS share path".to_string()))?;
    let allowed_roots = resolve_allowed_ios_share_roots(window)?;

    if allowed_roots
        .iter()
        .any(|root| canonical_file_path.starts_with(root))
    {
        return Ok(canonical_file_path);
    }

    Err(CommandError::BadRequest(format!(
        "iOS share path must be inside {}",
        Path::new(IOS_EXPORT_STAGING_ROOT_NAME).display()
    )))
}

#[tauri::command]
pub async fn ios_share_file(
    window: WebviewWindow,
    file_path: String,
) -> Result<IosShareFileResponse, CommandError> {
    log_command("ios_share_file");

    let file_path = file_path.trim();
    if file_path.is_empty() {
        return Err(CommandError::BadRequest("Missing file_path".to_string()));
    }

    let file_path = resolve_ios_shareable_file_path(&window, file_path)?;
    present_ios_share_sheet_for_path(&window, &file_path).await
}

#[tauri::command]
fn resolve_completed_export_archive_path(job_id: &str) -> Result<PathBuf, DomainError> {
    let status = get_data_archive_job_status_impl(job_id)?;
    if status.kind != "export" {
        return Err(DomainError::InvalidData("Invalid export job".to_string()));
    }

    if status.state != "completed" {
        return Err(DomainError::InvalidData(format!(
            "Export job is not completed yet: {}",
            job_id
        )));
    }

    let archive_path = status
        .result
        .and_then(|result| result.archive_path)
        .ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Export archive path is missing for job: {}",
                job_id
            ))
        })?;

    Ok(PathBuf::from(archive_path))
}

#[tauri::command]
pub async fn ios_share_export_data_archive(
    window: WebviewWindow,
    job_id: String,
) -> Result<IosShareExportArchiveResponse, CommandError> {
    log_command("ios_share_export_data_archive");

    let job_id = job_id.trim().to_string();
    if job_id.is_empty() {
        return Err(CommandError::BadRequest("Missing job_id".to_string()));
    }

    let archive_path = resolve_completed_export_archive_path(&job_id)
        .map_err(map_command_error("Failed to resolve export archive path"))?;

    let share_result = present_ios_share_sheet_for_path(&window, &archive_path).await?;

    let cleanup_error = cleanup_export_data_archive_impl(&job_id)
        .err()
        .map(|error| error.to_string());

    Ok(IosShareExportArchiveResponse {
        completed: share_result.completed,
        activity: share_result.activity,
        cleanup_error,
    })
}
