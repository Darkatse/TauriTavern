use chrono::Utc;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tauri::AppHandle;
use tauri::Manager;
use uuid::Uuid;

use crate::application::dto::data_archive_dto::{
    DATA_ARCHIVE_KIND_EXPORT, DATA_ARCHIVE_KIND_IMPORT, DATA_ARCHIVE_STATE_CANCELLED,
    DATA_ARCHIVE_STATE_COMPLETED, DATA_ARCHIVE_STATE_FAILED, DATA_ARCHIVE_STATE_PENDING,
    DATA_ARCHIVE_STATE_RUNNING, DataArchiveJobResult, DataArchiveJobStatus,
    UserBackupArchiveResult,
};
#[cfg(target_os = "ios")]
use crate::application::host_contract::IOS_EXPORT_STAGING_ROOT_NAME;
use crate::application::services::data_archive_service::DataArchiveJobBackend;
use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::domain::errors::DomainError;
use crate::infrastructure::paths::RuntimePaths;
use crate::infrastructure::persistence::file_system::DataDirectory;

use super::data_archive::{
    DataArchiveExportResult, DataArchiveImportResult, default_export_file_name, is_cancelled_error,
    run_export_data_archive, run_export_user_backup_archive, run_import_data_archive,
};

const EXPORT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) struct TauriDataArchiveJobBackend {
    app_handle: AppHandle,
    reconciler: Arc<dyn DataChangeReconciler>,
}

impl TauriDataArchiveJobBackend {
    pub(crate) fn new(app_handle: AppHandle, reconciler: Arc<dyn DataChangeReconciler>) -> Self {
        Self {
            app_handle,
            reconciler,
        }
    }
}

impl DataArchiveJobBackend for TauriDataArchiveJobBackend {
    fn start_import(
        &self,
        archive_path: &Path,
        archive_is_temporary: bool,
    ) -> Result<String, DomainError> {
        start_import_data_archive_job(
            &self.app_handle,
            self.reconciler.clone(),
            archive_path,
            archive_is_temporary,
        )
    }

    fn start_export(&self) -> Result<String, DomainError> {
        start_export_data_archive_job(&self.app_handle)
    }

    fn imports_root(&self) -> PathBuf {
        self.app_handle
            .state::<RuntimePaths>()
            .archive_imports_root
            .clone()
    }

    #[cfg(target_os = "ios")]
    fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        prepare_incoming_import_archive_path(&self.app_handle)
    }

    fn get_status(&self, job_id: &str) -> Result<DataArchiveJobStatus, DomainError> {
        get_data_archive_job_status(job_id)
    }

    fn cancel(&self, job_id: &str) -> Result<(), DomainError> {
        cancel_data_archive_job(job_id)
    }

    fn cleanup_export(&self, job_id: &str) -> Result<(), DomainError> {
        cleanup_export_data_archive(job_id)
    }

    fn save_export(&self, job_id: &str) -> Result<PathBuf, DomainError> {
        save_export_data_archive(&self.app_handle, job_id)
    }

    fn export_user_backup(
        &self,
        handle: &str,
        include_secrets: bool,
    ) -> Result<UserBackupArchiveResult, DomainError> {
        export_user_backup_archive_file(&self.app_handle, handle, include_secrets)
    }

    fn save_user_backup(
        &self,
        archive_path: &str,
        file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        save_user_backup_archive(&self.app_handle, archive_path, file_name)
    }

    fn cleanup_user_backup(&self, archive_path: &str) -> Result<(), DomainError> {
        cleanup_user_backup_archive(&self.app_handle, archive_path)
    }
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
                state: DATA_ARCHIVE_STATE_PENDING.to_string(),
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
            status.state = DATA_ARCHIVE_STATE_RUNNING.to_string();
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
            if status.state == DATA_ARCHIVE_STATE_PENDING {
                status.state = DATA_ARCHIVE_STATE_RUNNING.to_string();
            }
            if status.state != DATA_ARCHIVE_STATE_RUNNING {
                return;
            }
            status.stage = stage.to_string();
            status.progress_percent = progress_percent.clamp(0.0, 100.0);
            status.message = message.to_string();
        })
    }

    fn mark_completed_import(&self, result: DataArchiveImportResult) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_COMPLETED.to_string();
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
            status.state = DATA_ARCHIVE_STATE_COMPLETED.to_string();
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
            status.state = DATA_ARCHIVE_STATE_FAILED.to_string();
            status.stage = "failed".to_string();
            status.message = "Job failed".to_string();
            status.error = Some(error_message.to_string());
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn mark_cancelled(&self) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_CANCELLED.to_string();
            status.stage = "cancelled".to_string();
            status.message = "Job cancelled".to_string();
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::Relaxed);
        let _ = self.update_status(|status| {
            if status.state == DATA_ARCHIVE_STATE_PENDING
                || status.state == DATA_ARCHIVE_STATE_RUNNING
            {
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

fn start_import_data_archive_job(
    app_handle: &AppHandle,
    reconciler: Arc<dyn DataChangeReconciler>,
    archive_path: &Path,
    archive_is_temporary: bool,
) -> Result<String, DomainError> {
    if !archive_path.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Archive file does not exist: {}",
            archive_path.display()
        )));
    }

    let runtime_paths = app_handle.state::<RuntimePaths>();
    let imports_root = runtime_paths.archive_imports_root.clone();
    let data_root = runtime_paths.data_root.clone();
    fs::create_dir_all(&imports_root).map_err(|error| {
        DomainError::InternalError(format!("Failed to create job root: {}", error))
    })?;

    let job_id = Uuid::new_v4().simple().to_string();
    let job_root = imports_root.join(&job_id);
    fs::create_dir_all(&job_root).map_err(|error| {
        DomainError::InternalError(format!("Failed to create job workspace: {}", error))
    })?;

    let prepared_archive_path =
        prepare_import_archive_path(archive_path, &job_root, archive_is_temporary)?;

    let job = Arc::new(DataArchiveJob::new(&job_id, DATA_ARCHIVE_KIND_IMPORT));
    register_job(&job_id, job.clone())?;

    tauri::async_runtime::spawn(async move {
        let _ = job.mark_running("starting", "Import job started");

        let blocking_job = job.clone();
        let blocking_data_root = data_root.clone();
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
                &blocking_data_root,
                &blocking_archive,
                &blocking_job_root,
                &mut report_progress,
                &is_cancelled,
            )
        })
        .await;

        match blocking_result {
            Ok(Ok(result)) => {
                let initialize_result = DataDirectory::new(data_root.clone()).initialize().await;
                if let Err(error) = initialize_result {
                    let _ = job.mark_failed(&format!(
                        "Import completed but failed to initialize data directory: {}",
                        error
                    ));
                    cleanup_directory(&job_root);
                    return;
                }

                let refresh_result = reconciler.reconcile("import").await;
                if let Err(error) = refresh_result {
                    let _ = job.mark_failed(&format!(
                        "Import completed but failed to refresh runtime caches: {}",
                        error
                    ));
                    cleanup_directory(&job_root);
                    return;
                }

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

fn start_export_data_archive_job(app_handle: &AppHandle) -> Result<String, DomainError> {
    let runtime_paths = app_handle.state::<RuntimePaths>();
    let data_root = runtime_paths.data_root.clone();
    let export_root = runtime_paths.archive_exports_root.clone();
    fs::create_dir_all(&export_root).map_err(|error| {
        DomainError::InternalError(format!("Failed to create export directory: {}", error))
    })?;
    cleanup_stale_exports(&export_root);

    let job_id = Uuid::new_v4().simple().to_string();
    let job = Arc::new(DataArchiveJob::new(&job_id, DATA_ARCHIVE_KIND_EXPORT));
    register_job(&job_id, job.clone())?;

    let output_path = export_root.join(default_export_file_name());

    tauri::async_runtime::spawn(async move {
        let _ = job.mark_running("starting", "Export job started");

        let blocking_job = job.clone();
        let blocking_data_root = data_root.clone();
        let blocking_output = output_path.clone();

        let blocking_result = tauri::async_runtime::spawn_blocking(move || {
            let progress_job = blocking_job.clone();
            let mut report_progress = move |stage: &str, progress_percent: f32, message: &str| {
                let _ = progress_job.update_progress(stage, progress_percent, message);
            };

            let cancel_job = blocking_job.clone();
            let is_cancelled = move || cancel_job.is_cancel_requested();

            run_export_data_archive(
                &blocking_data_root,
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

fn export_user_backup_archive_file(
    app_handle: &AppHandle,
    handle: &str,
    include_secrets: bool,
) -> Result<UserBackupArchiveResult, DomainError> {
    let runtime_paths = app_handle.state::<RuntimePaths>();
    let export_root = resolve_user_backup_export_root(app_handle, &runtime_paths)?;
    fs::create_dir_all(&export_root).map_err(|error| {
        DomainError::InternalError(format!("Failed to create export directory: {}", error))
    })?;
    cleanup_stale_exports(&export_root);

    let (handle, user_root) = resolve_user_backup_root(&runtime_paths.data_root, handle)?;
    let file_name = default_user_backup_file_name(&handle);
    let output_path = export_root.join(format!(
        ".user-backup-{}-{}",
        Uuid::new_v4().simple(),
        file_name
    ));

    let mut report_progress = |_stage: &str, _progress_percent: f32, _message: &str| {};
    let is_cancelled = || false;

    if let Err(error) = run_export_user_backup_archive(
        &user_root,
        &output_path,
        include_secrets,
        &mut report_progress,
        &is_cancelled,
    ) {
        remove_file_if_exists(&output_path, "cleanup partial user backup archive");
        return Err(error);
    }

    Ok(UserBackupArchiveResult {
        file_name,
        archive_path: output_path.to_string_lossy().to_string(),
    })
}

fn get_data_archive_job_status(job_id: &str) -> Result<DataArchiveJobStatus, DomainError> {
    get_job(job_id)?.snapshot()
}

fn cancel_data_archive_job(job_id: &str) -> Result<(), DomainError> {
    let job = get_job(job_id)?;
    job.request_cancel();
    Ok(())
}

fn cleanup_export_data_archive(job_id: &str) -> Result<(), DomainError> {
    let status = get_job(job_id)?.snapshot()?;
    if status.kind != DATA_ARCHIVE_KIND_EXPORT || status.state != DATA_ARCHIVE_STATE_COMPLETED {
        return Err(DomainError::InvalidData(format!(
            "Export job is not completed: {}",
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

    remove_file_if_exists(Path::new(&archive_path), "cleanup export archive");
    Ok(())
}

fn save_export_data_archive(app_handle: &AppHandle, job_id: &str) -> Result<PathBuf, DomainError> {
    let status = get_job(job_id)?.snapshot()?;
    if status.kind != DATA_ARCHIVE_KIND_EXPORT || status.state != DATA_ARCHIVE_STATE_COMPLETED {
        return Err(DomainError::InvalidData(format!(
            "Export job is not completed: {}",
            job_id
        )));
    }

    let (archive_path, file_name) = status
        .result
        .and_then(|result| Some((result.archive_path?, result.file_name?)))
        .ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Export archive result is missing for job: {}",
                job_id
            ))
        })?;

    save_staged_archive_to_downloads(app_handle, Path::new(&archive_path), &file_name)
}

fn save_user_backup_archive(
    app_handle: &AppHandle,
    archive_path: &str,
    file_name: &str,
) -> Result<PathBuf, DomainError> {
    let source_path = resolve_staged_user_backup_archive_path(app_handle, archive_path)?;
    let file_name = validate_archive_file_name(file_name)?;
    save_staged_archive_to_downloads(app_handle, &source_path, &file_name)
}

fn cleanup_user_backup_archive(
    app_handle: &AppHandle,
    archive_path: &str,
) -> Result<(), DomainError> {
    let source_path = resolve_staged_user_backup_archive_path(app_handle, archive_path)?;
    remove_file_if_exists(&source_path, "cleanup user backup archive");
    Ok(())
}

fn save_staged_archive_to_downloads(
    app_handle: &AppHandle,
    source_path: &Path,
    file_name: &str,
) -> Result<PathBuf, DomainError> {
    if cfg!(target_os = "android") {
        return Err(DomainError::InternalError(
            "Android archive exports must use the native document save bridge".to_string(),
        ));
    }

    if !source_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "Export archive file not found: {}",
            source_path.display()
        )));
    }

    let download_dir = app_handle.path().download_dir().map_err(|error| {
        DomainError::InternalError(format!("Failed to resolve downloads directory: {}", error))
    })?;
    fs::create_dir_all(&download_dir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create downloads directory {}: {}",
            download_dir.display(),
            error
        ))
    })?;

    let target_path = download_dir.join(file_name);
    if target_path.exists() {
        return Err(DomainError::InvalidData(format!(
            "Export target already exists: {}",
            target_path.display()
        )));
    }

    if fs::rename(source_path, &target_path).is_ok() {
        return Ok(target_path);
    }

    if let Err(error) = fs::copy(source_path, &target_path) {
        remove_file_if_exists(&target_path, "cleanup partial export save");
        return Err(DomainError::InternalError(format!(
            "Failed to save export archive {} to {}: {}",
            source_path.display(),
            target_path.display(),
            error
        )));
    }

    if let Err(error) = fs::remove_file(source_path) {
        remove_file_if_exists(&target_path, "cleanup partial export save");
        return Err(DomainError::InternalError(format!(
            "Failed to remove staged export archive {}: {}",
            source_path.display(),
            error
        )));
    }

    Ok(target_path)
}

fn validate_archive_file_name(file_name: &str) -> Result<String, DomainError> {
    let file_name = file_name.trim();
    if file_name.is_empty() {
        return Err(DomainError::InvalidData(
            "Export archive filename is required".to_string(),
        ));
    }

    if file_name.contains('/') || file_name.contains('\\') {
        return Err(DomainError::InvalidData(format!(
            "Invalid export archive filename: {}",
            file_name
        )));
    }

    let mut components = Path::new(file_name).components();
    let component = components.next();
    if !matches!(component, Some(Component::Normal(_))) || components.next().is_some() {
        return Err(DomainError::InvalidData(format!(
            "Invalid export archive filename: {}",
            file_name
        )));
    }

    Ok(file_name.to_string())
}

#[cfg(target_os = "ios")]
fn candidate_user_backup_export_roots(
    app_handle: &AppHandle,
    _runtime_paths: &RuntimePaths,
) -> Result<Vec<PathBuf>, DomainError> {
    let path_resolver = app_handle.path();
    let mut roots = Vec::new();

    if let Ok(cache_dir) = path_resolver.app_cache_dir() {
        roots.push(
            cache_dir
                .join(IOS_EXPORT_STAGING_ROOT_NAME)
                .join("user-backups"),
        );
    }

    if let Ok(temp_dir) = path_resolver.temp_dir() {
        roots.push(
            temp_dir
                .join(IOS_EXPORT_STAGING_ROOT_NAME)
                .join("user-backups"),
        );
    }

    if roots.is_empty() {
        return Err(DomainError::InternalError(
            "No writable iOS user backup staging directory is available".to_string(),
        ));
    }

    Ok(roots)
}

#[cfg(not(target_os = "ios"))]
fn candidate_user_backup_export_roots(
    _app_handle: &AppHandle,
    runtime_paths: &RuntimePaths,
) -> Result<Vec<PathBuf>, DomainError> {
    Ok(vec![runtime_paths.archive_exports_root.clone()])
}

fn resolve_user_backup_export_root(
    app_handle: &AppHandle,
    runtime_paths: &RuntimePaths,
) -> Result<PathBuf, DomainError> {
    let roots = candidate_user_backup_export_roots(app_handle, runtime_paths)?;
    roots.into_iter().next().ok_or_else(|| {
        DomainError::InternalError(
            "No writable user backup staging directory is available".to_string(),
        )
    })
}

fn resolve_staged_user_backup_archive_path(
    app_handle: &AppHandle,
    archive_path: &str,
) -> Result<PathBuf, DomainError> {
    let archive_path = archive_path.trim();
    if archive_path.is_empty() {
        return Err(DomainError::InvalidData(
            "User backup archive path is required".to_string(),
        ));
    }

    let requested_path = PathBuf::from(archive_path);
    if !requested_path.is_absolute() {
        return Err(DomainError::InvalidData(
            "User backup archive path must be absolute".to_string(),
        ));
    }

    let canonical_path = fs::canonicalize(&requested_path).map_err(|_| {
        DomainError::NotFound(format!(
            "User backup archive file not found: {}",
            requested_path.display()
        ))
    })?;
    if !canonical_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "User backup archive file not found: {}",
            canonical_path.display()
        )));
    }

    let runtime_paths = app_handle.state::<RuntimePaths>();
    let roots = candidate_user_backup_export_roots(app_handle, &runtime_paths)?;
    let mut canonical_roots = Vec::new();
    for root in roots {
        match fs::canonicalize(&root) {
            Ok(root) => canonical_roots.push(root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to resolve user backup staging directory {}: {}",
                    root.display(),
                    error
                )));
            }
        }
    }

    if canonical_roots
        .iter()
        .any(|root| canonical_path.starts_with(root))
    {
        return Ok(canonical_path);
    }

    Err(DomainError::InvalidData(format!(
        "User backup archive path is outside the staging directory: {}",
        requested_path.display()
    )))
}

fn resolve_user_backup_root(
    data_root: &Path,
    handle: &str,
) -> Result<(String, PathBuf), DomainError> {
    let handle = handle.trim();
    if handle.is_empty() {
        return Err(DomainError::InvalidData(
            "User handle is required for backup".to_string(),
        ));
    }

    if handle.contains('/') || handle.contains('\\') {
        return Err(DomainError::InvalidData(format!(
            "Invalid user handle for backup: {}",
            handle
        )));
    }

    let mut components = Path::new(handle).components();
    let component = components.next();
    if !matches!(component, Some(Component::Normal(_))) || components.next().is_some() {
        return Err(DomainError::InvalidData(format!(
            "Invalid user handle for backup: {}",
            handle
        )));
    }

    let user_root = data_root.join(handle);
    if !user_root.is_dir() {
        return Err(DomainError::NotFound(format!(
            "User directory not found: {}",
            handle
        )));
    }

    Ok((handle.to_string(), user_root))
}

fn default_user_backup_file_name(handle: &str) -> String {
    format!("{}-{}.zip", handle, Utc::now().format("%Y%m%d-%H%M%S"))
}

#[cfg(target_os = "ios")]
fn prepare_incoming_import_archive_path(app_handle: &AppHandle) -> Result<PathBuf, DomainError> {
    let imports_root = app_handle
        .state::<RuntimePaths>()
        .archive_imports_root
        .clone();
    let incoming_dir = imports_root.join("incoming");
    fs::create_dir_all(&incoming_dir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create import staging directory {}: {}",
            incoming_dir.display(),
            error
        ))
    })?;

    Ok(incoming_dir.join(format!(
        "tauritavern-import-{}.archive",
        Uuid::new_v4().simple()
    )))
}

fn prepare_import_archive_path(
    source_archive_path: &Path,
    job_root: &Path,
    archive_is_temporary: bool,
) -> Result<PathBuf, DomainError> {
    if !archive_is_temporary {
        return Ok(source_archive_path.to_path_buf());
    }

    let staged_archive_path = job_root.join("import.archive");
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
            tracing::warn!("Failed to {} {}: {}", operation, path.display(), error);
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
