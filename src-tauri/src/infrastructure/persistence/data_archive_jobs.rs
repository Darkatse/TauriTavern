use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tauri::AppHandle;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::infrastructure::paths::resolve_app_data_dir;

use super::data_archive::{
    default_export_file_name, is_cancelled_error, run_export_data_archive, run_import_data_archive,
    DataArchiveExportResult, DataArchiveImportResult,
};

const STATE_PENDING: &str = "pending";
const STATE_RUNNING: &str = "running";
const STATE_COMPLETED: &str = "completed";
const STATE_FAILED: &str = "failed";
const STATE_CANCELLED: &str = "cancelled";

const KIND_IMPORT: &str = "import";
const KIND_EXPORT: &str = "export";

const EXPORT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct DataArchiveJobResult {
    pub source_users: Vec<String>,
    pub target_user: Option<String>,
    pub file_name: Option<String>,
    pub archive_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataArchiveJobStatus {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub stage: String,
    pub progress_percent: f32,
    pub message: String,
    pub result: Option<DataArchiveJobResult>,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

struct DataArchiveJob {
    status: Mutex<DataArchiveJobStatus>,
    cancel_requested: AtomicBool,
}

impl DataArchiveJob {
    fn new(job_id: &str, kind: &str) -> Self {
        Self {
            status: Mutex::new(DataArchiveJobStatus {
                job_id: job_id.to_string(),
                kind: kind.to_string(),
                state: STATE_PENDING.to_string(),
                stage: "queued".to_string(),
                progress_percent: 0.0,
                message: "Job queued".to_string(),
                result: None,
                error: None,
                started_at: Utc::now().to_rfc3339(),
                finished_at: None,
            }),
            cancel_requested: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> Result<DataArchiveJobStatus, DomainError> {
        let status = self
            .status
            .lock()
            .map_err(|_| DomainError::InternalError("Failed to lock job status".to_string()))?;
        Ok(status.clone())
    }

    fn mark_running(&self, stage: &str, message: &str) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = STATE_RUNNING.to_string();
            status.stage = stage.to_string();
            status.message = message.to_string();
            status.progress_percent = status.progress_percent.clamp(0.0, 100.0);
            status.error = None;
        })
    }

    fn update_progress(
        &self,
        stage: &str,
        progress_percent: f32,
        message: &str,
    ) -> Result<(), DomainError> {
        self.update_status(|status| {
            if status.state == STATE_PENDING {
                status.state = STATE_RUNNING.to_string();
            }
            if status.state != STATE_RUNNING {
                return;
            }
            status.stage = stage.to_string();
            status.progress_percent = progress_percent.clamp(0.0, 100.0);
            status.message = message.to_string();
        })
    }

    fn mark_completed_import(&self, result: DataArchiveImportResult) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = STATE_COMPLETED.to_string();
            status.stage = "completed".to_string();
            status.progress_percent = 100.0;
            status.message = "Import completed".to_string();
            status.result = Some(DataArchiveJobResult {
                source_users: result.source_users,
                target_user: Some(result.target_user),
                file_name: None,
                archive_path: None,
            });
            status.error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn mark_completed_export(&self, result: DataArchiveExportResult) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = STATE_COMPLETED.to_string();
            status.stage = "completed".to_string();
            status.progress_percent = 100.0;
            status.message = "Export completed".to_string();
            status.result = Some(DataArchiveJobResult {
                source_users: Vec::new(),
                target_user: None,
                file_name: Some(result.file_name),
                archive_path: Some(result.archive_path.to_string_lossy().to_string()),
            });
            status.error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn mark_failed(&self, error_message: &str) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = STATE_FAILED.to_string();
            status.stage = "failed".to_string();
            status.message = "Job failed".to_string();
            status.error = Some(error_message.to_string());
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn mark_cancelled(&self) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = STATE_CANCELLED.to_string();
            status.stage = "cancelled".to_string();
            status.message = "Job cancelled".to_string();
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::Relaxed);
        let _ = self.update_status(|status| {
            if status.state == STATE_PENDING || status.state == STATE_RUNNING {
                status.message = "Cancellation requested".to_string();
            }
        });
    }

    fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::Relaxed)
    }

    fn update_status(
        &self,
        update: impl FnOnce(&mut DataArchiveJobStatus),
    ) -> Result<(), DomainError> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| DomainError::InternalError("Failed to lock job status".to_string()))?;
        update(&mut status);
        Ok(())
    }
}

static JOBS: OnceLock<Mutex<HashMap<String, Arc<DataArchiveJob>>>> = OnceLock::new();

fn jobs_registry() -> &'static Mutex<HashMap<String, Arc<DataArchiveJob>>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_job(job_id: &str) -> Result<Arc<DataArchiveJob>, DomainError> {
    let registry = jobs_registry()
        .lock()
        .map_err(|_| DomainError::InternalError("Failed to lock job registry".to_string()))?;

    registry
        .get(job_id)
        .cloned()
        .ok_or_else(|| DomainError::NotFound(format!("Data archive job not found: {}", job_id)))
}

fn register_job(job_id: &str, job: Arc<DataArchiveJob>) -> Result<(), DomainError> {
    let mut registry = jobs_registry()
        .lock()
        .map_err(|_| DomainError::InternalError("Failed to lock job registry".to_string()))?;
    registry.insert(job_id.to_string(), job);
    Ok(())
}

pub fn start_import_data_archive_job(
    app_handle: &AppHandle,
    archive_path: &Path,
    archive_is_temporary: bool,
) -> Result<String, DomainError> {
    if !archive_path.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Archive file does not exist: {}",
            archive_path.display()
        )));
    }

    let app_data_dir = resolve_app_data_dir(app_handle).map_err(|error| {
        DomainError::InternalError(format!("Failed to resolve app data directory: {}", error))
    })?;
    let jobs_root = app_data_dir.join(".data-archive-jobs");
    fs::create_dir_all(&jobs_root).map_err(|error| {
        DomainError::InternalError(format!("Failed to create job root: {}", error))
    })?;

    let job_id = Uuid::new_v4().simple().to_string();
    let job_root = jobs_root.join(&job_id);
    fs::create_dir_all(&job_root).map_err(|error| {
        DomainError::InternalError(format!("Failed to create job workspace: {}", error))
    })?;

    let prepared_archive_path =
        prepare_import_archive_path(archive_path, &job_root, archive_is_temporary)?;

    let job = Arc::new(DataArchiveJob::new(&job_id, KIND_IMPORT));
    register_job(&job_id, job.clone())?;

    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let _ = job.mark_running("starting", "Import job started");

        let blocking_job = job.clone();
        let blocking_app = app.clone();
        let blocking_archive = prepared_archive_path.clone();
        let blocking_job_root = job_root.clone();

        let blocking_result = tauri::async_runtime::spawn_blocking(move || {
            let progress_job = blocking_job.clone();
            let mut report_progress = move |stage: &str, progress_percent: f32, message: &str| {
                let _ = progress_job.update_progress(stage, progress_percent, message);
            };

            let cancel_job = blocking_job.clone();
            let is_cancelled = move || cancel_job.is_cancel_requested();

            run_import_data_archive(
                &blocking_app,
                &blocking_archive,
                &blocking_job_root,
                &mut report_progress,
                &is_cancelled,
            )
        })
        .await;

        match blocking_result {
            Ok(Ok(result)) => {
                let _ = job.mark_completed_import(result);
            }
            Ok(Err(error)) => {
                if job.is_cancel_requested() || is_cancelled_error(&error) {
                    let _ = job.mark_cancelled();
                } else {
                    let _ = job.mark_failed(&error.to_string());
                }
            }
            Err(error) => {
                let _ = job.mark_failed(&format!("Import task join error: {}", error));
            }
        }

        cleanup_directory(&job_root);
    });

    Ok(job_id)
}

pub fn start_export_data_archive_job(app_handle: &AppHandle) -> Result<String, DomainError> {
    let app_data_dir = resolve_app_data_dir(app_handle).map_err(|error| {
        DomainError::InternalError(format!("Failed to resolve app data directory: {}", error))
    })?;

    let export_root = app_data_dir.join(".data-archive-exports");
    fs::create_dir_all(&export_root).map_err(|error| {
        DomainError::InternalError(format!("Failed to create export directory: {}", error))
    })?;
    cleanup_stale_exports(&export_root);

    let job_id = Uuid::new_v4().simple().to_string();
    let job = Arc::new(DataArchiveJob::new(&job_id, KIND_EXPORT));
    register_job(&job_id, job.clone())?;

    let output_path = export_root.join(default_export_file_name());
    let app = app_handle.clone();

    tauri::async_runtime::spawn(async move {
        let _ = job.mark_running("starting", "Export job started");

        let blocking_job = job.clone();
        let blocking_app = app.clone();
        let blocking_output = output_path.clone();

        let blocking_result = tauri::async_runtime::spawn_blocking(move || {
            let progress_job = blocking_job.clone();
            let mut report_progress = move |stage: &str, progress_percent: f32, message: &str| {
                let _ = progress_job.update_progress(stage, progress_percent, message);
            };

            let cancel_job = blocking_job.clone();
            let is_cancelled = move || cancel_job.is_cancel_requested();

            run_export_data_archive(
                &blocking_app,
                &blocking_output,
                &mut report_progress,
                &is_cancelled,
            )
        })
        .await;

        match blocking_result {
            Ok(Ok(result)) => {
                let _ = job.mark_completed_export(result);
            }
            Ok(Err(error)) => {
                if job.is_cancel_requested() || is_cancelled_error(&error) {
                    let _ = job.mark_cancelled();
                } else {
                    let _ = job.mark_failed(&error.to_string());
                }

                remove_file_if_exists(&output_path, "cleanup partial export archive");
            }
            Err(error) => {
                let _ = job.mark_failed(&format!("Export task join error: {}", error));
                remove_file_if_exists(&output_path, "cleanup partial export archive");
            }
        }
    });

    Ok(job_id)
}

pub fn get_data_archive_job_status(job_id: &str) -> Result<DataArchiveJobStatus, DomainError> {
    get_job(job_id)?.snapshot()
}

pub fn cancel_data_archive_job(job_id: &str) -> Result<(), DomainError> {
    let job = get_job(job_id)?;
    job.request_cancel();
    Ok(())
}

fn prepare_import_archive_path(
    source_archive_path: &Path,
    job_root: &Path,
    archive_is_temporary: bool,
) -> Result<PathBuf, DomainError> {
    if !archive_is_temporary {
        return Ok(source_archive_path.to_path_buf());
    }

    let staged_archive_path = job_root.join("import.zip");
    if fs::rename(source_archive_path, &staged_archive_path).is_ok() {
        return Ok(staged_archive_path);
    }

    fs::copy(source_archive_path, &staged_archive_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to copy temporary archive to job workspace: {}",
            error
        ))
    })?;

    if let Err(remove_error) = fs::remove_file(source_archive_path) {
        if remove_error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Failed to remove temporary source archive {}: {}",
                source_archive_path.display(),
                remove_error
            );
        }
    }

    Ok(staged_archive_path)
}

fn cleanup_directory(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("Failed to cleanup directory {}: {}", path.display(), error);
        }
    }
}

fn remove_file_if_exists(path: &Path, operation: &str) {
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Failed to {} {}: {}",
                operation,
                path.display(),
                error
            );
        }
    }
}

fn cleanup_stale_exports(export_root: &Path) {
    let Ok(entries) = fs::read_dir(export_root) else {
        return;
    };

    let now = SystemTime::now();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        let Ok(modified) = metadata.modified() else {
            continue;
        };

        let Ok(age) = now.duration_since(modified) else {
            continue;
        };

        if age <= EXPORT_RETENTION {
            continue;
        }

        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to remove stale export {}: {}",
                    path.display(),
                    error
                );
            }
        }
    }
}
