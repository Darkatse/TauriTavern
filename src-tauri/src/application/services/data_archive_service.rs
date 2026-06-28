use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::application::dto::data_archive_dto::{
    DATA_ARCHIVE_ARTIFACT_AVAILABLE, DATA_ARCHIVE_ARTIFACT_DISPOSED, DATA_ARCHIVE_ARTIFACT_MISSING,
    DATA_ARCHIVE_KIND_EXPORT, DATA_ARCHIVE_KIND_IMPORT, DATA_ARCHIVE_STATE_CANCELLED,
    DATA_ARCHIVE_STATE_COMPLETED, DATA_ARCHIVE_STATE_FAILED, DATA_ARCHIVE_STATE_PENDING,
    DATA_ARCHIVE_STATE_RUNNING, DataArchiveJobResult, DataArchiveJobStatus,
    UserBackupArchiveResult,
};
use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::domain::errors::DomainError;
use crate::domain::models::data_archive::{
    DataArchiveImportFailure, DataArchiveLocalMutationSummary,
};

#[derive(Default)]
pub(crate) struct DataArchiveJobRegistry {
    jobs: Mutex<HashMap<String, Arc<DataArchiveJobHandle>>>,
}

impl DataArchiveJobRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(
        &self,
        job_id: &str,
        job: Arc<DataArchiveJobHandle>,
    ) -> Result<(), DomainError> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| DomainError::InternalError("Failed to lock job registry".to_string()))?;
        jobs.insert(job_id.to_string(), job);
        Ok(())
    }

    pub(crate) fn get(&self, job_id: &str) -> Result<Arc<DataArchiveJobHandle>, DomainError> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| DomainError::InternalError("Failed to lock job registry".to_string()))?;
        jobs.get(job_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("Data archive job not found: {}", job_id)))
    }

    pub(crate) fn protected_export_artifact_paths(&self) -> Result<Vec<PathBuf>, DomainError> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| DomainError::InternalError("Failed to lock job registry".to_string()))?;
        let handles = jobs.values().cloned().collect::<Vec<_>>();
        drop(jobs);

        let mut paths = Vec::new();
        for job in handles {
            if let Some(path) = job.protected_export_artifact_path()? {
                paths.push(path);
            }
        }
        Ok(paths)
    }
}

impl Drop for DataArchiveJobRegistry {
    fn drop(&mut self) {
        if let Ok(jobs) = self.jobs.get_mut() {
            for job in jobs.values() {
                job.request_cancel();
            }
        }
    }
}

pub(crate) struct DataArchiveJobHandle {
    status: Mutex<DataArchiveJobStatus>,
    export_artifact_path: Mutex<Option<PathBuf>>,
    cancel_requested: AtomicBool,
}

impl DataArchiveJobHandle {
    pub(crate) fn new(job_id: &str, kind: &str) -> Self {
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
                local_applied: None,
                reconcile_error: None,
                started_at: Utc::now().to_rfc3339(),
                finished_at: None,
            }),
            export_artifact_path: Mutex::new(None),
            cancel_requested: AtomicBool::new(false),
        }
    }

    pub(crate) fn new_export(job_id: &str, artifact_path: PathBuf) -> Self {
        Self {
            export_artifact_path: Mutex::new(Some(artifact_path)),
            ..Self::new(job_id, DATA_ARCHIVE_KIND_EXPORT)
        }
    }

    pub(crate) fn snapshot(&self) -> Result<DataArchiveJobStatus, DomainError> {
        let status = self
            .status
            .lock()
            .map_err(|_| DomainError::InternalError("Failed to lock job status".to_string()))?;
        Ok(status.clone())
    }

    pub(crate) fn mark_running(&self, stage: &str, message: &str) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_RUNNING.to_string();
            status.stage = stage.to_string();
            status.message = message.to_string();
            status.progress_percent = status.progress_percent.clamp(0.0, 100.0);
            status.error = None;
        })
    }

    pub(crate) fn update_progress(
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

    pub(crate) fn mark_completed_import(
        &self,
        source_users: Vec<String>,
        target_user: String,
    ) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_COMPLETED.to_string();
            status.stage = "completed".to_string();
            status.progress_percent = 100.0;
            status.message = "Import completed".to_string();
            status.result = Some(DataArchiveJobResult {
                source_users,
                target_user: Some(target_user),
                file_name: None,
                archive_path: None,
                artifact_state: None,
                saved_path: None,
            });
            status.error = None;
            status.local_applied = None;
            status.reconcile_error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_completed_export(
        &self,
        file_name: String,
        archive_path: PathBuf,
    ) -> Result<(), DomainError> {
        self.set_export_artifact_path(archive_path.clone())?;
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_COMPLETED.to_string();
            status.stage = "completed".to_string();
            status.progress_percent = 100.0;
            status.message = "Export completed".to_string();
            status.result = Some(DataArchiveJobResult {
                source_users: Vec::new(),
                target_user: None,
                file_name: Some(file_name),
                archive_path: Some(archive_path.to_string_lossy().to_string()),
                artifact_state: Some(DATA_ARCHIVE_ARTIFACT_AVAILABLE.to_string()),
                saved_path: None,
            });
            status.error = None;
            status.local_applied = None;
            status.reconcile_error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_failed(&self, error_message: &str) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_FAILED.to_string();
            status.stage = "failed".to_string();
            status.message = "Job failed".to_string();
            status.error = Some(error_message.to_string());
            status.local_applied = None;
            status.reconcile_error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_failed_after_local_mutation(
        &self,
        error_message: &str,
        local_applied: DataArchiveLocalMutationSummary,
        reconcile_error: Option<String>,
    ) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_FAILED.to_string();
            status.stage = "failed".to_string();
            status.message = "Job failed".to_string();
            status.error = Some(error_message.to_string());
            status.local_applied = Some(local_applied.into());
            status.reconcile_error = reconcile_error;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_cancelled(&self) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_CANCELLED.to_string();
            status.stage = "cancelled".to_string();
            status.message = "Job cancelled".to_string();
            status.local_applied = None;
            status.reconcile_error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_cancelled_after_local_mutation(
        &self,
        local_applied: DataArchiveLocalMutationSummary,
        reconcile_error: Option<String>,
    ) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_CANCELLED.to_string();
            status.stage = "cancelled".to_string();
            status.message = "Job cancelled".to_string();
            status.local_applied = Some(local_applied.into());
            status.reconcile_error = reconcile_error;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_export_artifact_disposed(
        &self,
        saved_target: Option<String>,
    ) -> Result<(), DomainError> {
        self.clear_export_artifact_path()?;
        self.update_status(|status| {
            if let Some(result) = status.result.as_mut() {
                result.artifact_state = Some(DATA_ARCHIVE_ARTIFACT_DISPOSED.to_string());
                result.saved_path = saved_target;
            }
        })
    }

    pub(crate) fn mark_export_artifact_missing(&self) -> Result<(), DomainError> {
        self.clear_export_artifact_path()?;
        self.update_status(|status| {
            if let Some(result) = status.result.as_mut() {
                result.artifact_state = Some(DATA_ARCHIVE_ARTIFACT_MISSING.to_string());
                result.saved_path = None;
            }
        })
    }

    pub(crate) fn protected_export_artifact_path(&self) -> Result<Option<PathBuf>, DomainError> {
        let status = self.snapshot()?;
        if status.kind != DATA_ARCHIVE_KIND_EXPORT
            || status.state == DATA_ARCHIVE_STATE_FAILED
            || status.state == DATA_ARCHIVE_STATE_CANCELLED
        {
            return Ok(None);
        }

        if let Some(result) = status.result.as_ref() {
            let artifact_state = result.artifact_state.as_deref();
            if matches!(
                artifact_state,
                Some(DATA_ARCHIVE_ARTIFACT_DISPOSED | DATA_ARCHIVE_ARTIFACT_MISSING)
            ) {
                return Ok(None);
            }
            if status.state == DATA_ARCHIVE_STATE_COMPLETED {
                return Ok(result.archive_path.as_ref().map(PathBuf::from));
            }
        }

        let path = self.export_artifact_path.lock().map_err(|_| {
            DomainError::InternalError("Failed to lock export artifact path".to_string())
        })?;
        Ok(path.clone())
    }

    fn set_export_artifact_path(&self, artifact_path: PathBuf) -> Result<(), DomainError> {
        let mut path = self.export_artifact_path.lock().map_err(|_| {
            DomainError::InternalError("Failed to lock export artifact path".to_string())
        })?;
        *path = Some(artifact_path);
        Ok(())
    }

    fn claim_export_artifact_path(&self) -> Result<Option<PathBuf>, DomainError> {
        let mut path = self.export_artifact_path.lock().map_err(|_| {
            DomainError::InternalError("Failed to lock export artifact path".to_string())
        })?;
        Ok(path.take())
    }

    fn restore_export_artifact_path(&self, artifact_path: PathBuf) -> Result<(), DomainError> {
        let mut path = self.export_artifact_path.lock().map_err(|_| {
            DomainError::InternalError("Failed to lock export artifact path".to_string())
        })?;
        if path.is_none() {
            *path = Some(artifact_path);
        }
        Ok(())
    }

    pub(crate) fn clear_export_artifact_path(&self) -> Result<(), DomainError> {
        let mut path = self.export_artifact_path.lock().map_err(|_| {
            DomainError::InternalError("Failed to lock export artifact path".to_string())
        })?;
        *path = None;
        Ok(())
    }

    pub(crate) fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::Relaxed);
        let _ = self.update_status(|status| {
            if status.state == DATA_ARCHIVE_STATE_PENDING
                || status.state == DATA_ARCHIVE_STATE_RUNNING
            {
                status.message = "Cancellation requested".to_string();
            }
        });
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
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

pub(crate) struct ImportArchiveExecutionRequest {
    pub data_root: PathBuf,
    pub archive_path: PathBuf,
    pub workspace_root: PathBuf,
}

pub(crate) struct ExportArchiveExecutionRequest {
    pub data_root: PathBuf,
    pub output_path: PathBuf,
    pub file_name: String,
}

pub(crate) struct UserBackupArchiveExecutionRequest {
    pub user_root: PathBuf,
    pub output_path: PathBuf,
    pub include_secrets: bool,
}

pub(crate) struct UserBackupArchiveTarget {
    pub file_name: String,
    pub request: UserBackupArchiveExecutionRequest,
}

pub(crate) struct ArchiveImportExecutionReport {
    pub source_users: Vec<String>,
    pub target_user: String,
    pub local_applied: DataArchiveLocalMutationSummary,
}

pub(crate) struct ArchiveExportExecutionReport {
    pub file_name: String,
    pub archive_path: PathBuf,
}

pub(crate) trait DataArchiveExecutor: Send + Sync {
    fn import_full_data(
        &self,
        request: ImportArchiveExecutionRequest,
        report_progress: &mut dyn FnMut(&str, f32, &str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveImportExecutionReport, DataArchiveImportFailure>;

    fn export_full_data(
        &self,
        request: ExportArchiveExecutionRequest,
        report_progress: &mut dyn FnMut(&str, f32, &str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ArchiveExportExecutionReport, DomainError>;

    fn export_user_backup(
        &self,
        request: UserBackupArchiveExecutionRequest,
        report_progress: &mut dyn FnMut(&str, f32, &str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<(), DomainError>;
}

pub(crate) trait DataArchiveFileGateway: Send + Sync {
    fn imports_root(&self) -> PathBuf;
    #[cfg(target_os = "ios")]
    fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError>;
    fn prepare_import_archive(
        &self,
        archive_path: &Path,
        archive_is_temporary: bool,
        job_id: &str,
    ) -> Result<ImportArchiveExecutionRequest, DomainError>;
    fn prepare_export_archive(
        &self,
        job_id: &str,
        protected_paths: &[PathBuf],
    ) -> Result<ExportArchiveExecutionRequest, DomainError>;
    fn prepare_user_backup_archive(
        &self,
        handle: &str,
        include_secrets: bool,
        protected_paths: &[PathBuf],
    ) -> Result<UserBackupArchiveTarget, DomainError>;
    fn cleanup_directory(&self, path: &Path);
    fn cleanup_export(&self, archive_path: &Path) -> Result<(), DomainError>;
    fn save_export(&self, archive_path: &Path, file_name: &str) -> Result<PathBuf, DomainError>;
    fn save_user_backup(&self, archive_path: &str, file_name: &str)
    -> Result<PathBuf, DomainError>;
    fn cleanup_user_backup(&self, archive_path: &str) -> Result<(), DomainError>;
}

struct CompletedExportJob {
    job: Arc<DataArchiveJobHandle>,
    result: DataArchiveJobResult,
}

struct CompletedExportArtifact {
    job: Arc<DataArchiveJobHandle>,
    archive_path: PathBuf,
    file_name: String,
}

#[async_trait]
pub(crate) trait DataRootInitializer: Send + Sync {
    async fn initialize_data_root(&self, data_root: &Path) -> Result<(), DomainError>;
}

pub struct DataArchiveService {
    jobs: Arc<DataArchiveJobRegistry>,
    executor: Arc<dyn DataArchiveExecutor>,
    files: Arc<dyn DataArchiveFileGateway>,
    data_root_initializer: Arc<dyn DataRootInitializer>,
    reconciler: Arc<dyn DataChangeReconciler>,
}

impl DataArchiveService {
    pub(crate) fn new(
        jobs: Arc<DataArchiveJobRegistry>,
        executor: Arc<dyn DataArchiveExecutor>,
        files: Arc<dyn DataArchiveFileGateway>,
        data_root_initializer: Arc<dyn DataRootInitializer>,
        reconciler: Arc<dyn DataChangeReconciler>,
    ) -> Self {
        Self {
            jobs,
            executor,
            files,
            data_root_initializer,
            reconciler,
        }
    }

    pub fn start_import(
        &self,
        archive_path: &Path,
        archive_is_temporary: bool,
    ) -> Result<String, DomainError> {
        let job_id = Uuid::new_v4().simple().to_string();
        let request =
            self.files
                .prepare_import_archive(archive_path, archive_is_temporary, &job_id)?;
        let workspace_root = request.workspace_root.clone();
        let data_root = request.data_root.clone();
        let job = Arc::new(DataArchiveJobHandle::new(&job_id, DATA_ARCHIVE_KIND_IMPORT));
        if let Err(error) = self.jobs.insert(&job_id, job.clone()) {
            self.files.cleanup_directory(&workspace_root);
            return Err(error);
        }

        let executor = self.executor.clone();
        let files = self.files.clone();
        let data_root_initializer = self.data_root_initializer.clone();
        let reconciler = self.reconciler.clone();

        tokio::spawn(async move {
            let _ = job.mark_running("starting", "Import job started");

            let blocking_job = job.clone();
            let blocking_result = tokio::task::spawn_blocking(move || {
                let progress_job = blocking_job.clone();
                let mut report_progress =
                    move |stage: &str, progress_percent: f32, message: &str| {
                        let _ = progress_job.update_progress(stage, progress_percent, message);
                    };

                let cancel_job = blocking_job.clone();
                let is_cancelled = move || cancel_job.is_cancel_requested();

                executor.import_full_data(request, &mut report_progress, &is_cancelled)
            })
            .await;

            match blocking_result {
                Ok(Ok(result)) => {
                    if result.local_applied.changed() {
                        if let Err(error) = reconcile_import_data_change(
                            &data_root_initializer,
                            &reconciler,
                            &data_root,
                        )
                        .await
                        {
                            let _ = job.mark_failed_after_local_mutation(
                                &format!("Import completed but {}", error),
                                result.local_applied,
                                Some(error),
                            );
                            return;
                        }
                    }
                    let _ = job.mark_completed_import(result.source_users, result.target_user);
                }
                Ok(Err(failure)) => {
                    let cancelled = job.is_cancel_requested() || is_cancelled_error(&failure.error);
                    if failure.local_applied.changed() {
                        let reconcile_error = reconcile_import_data_change(
                            &data_root_initializer,
                            &reconciler,
                            &data_root,
                        )
                        .await
                        .err();

                        if cancelled {
                            let _ = job.mark_cancelled_after_local_mutation(
                                failure.local_applied,
                                reconcile_error,
                            );
                        } else {
                            let _ = job.mark_failed_after_local_mutation(
                                &failure.error.to_string(),
                                failure.local_applied,
                                reconcile_error,
                            );
                        }
                    } else if cancelled {
                        let _ = job.mark_cancelled();
                    } else {
                        let _ = job.mark_failed(&failure.error.to_string());
                    }
                }
                Err(error) => {
                    let _ = job.mark_failed(&format!("Import task join error: {}", error));
                }
            }

            files.cleanup_directory(&workspace_root);
        });

        Ok(job_id)
    }

    pub fn start_export(&self) -> Result<String, DomainError> {
        let job_id = Uuid::new_v4().simple().to_string();
        let protected_paths = self.jobs.protected_export_artifact_paths()?;
        let request = self
            .files
            .prepare_export_archive(&job_id, &protected_paths)?;
        let output_path = request.output_path.clone();
        let job = Arc::new(DataArchiveJobHandle::new_export(
            &job_id,
            output_path.clone(),
        ));
        self.jobs.insert(&job_id, job.clone())?;

        let executor = self.executor.clone();
        let files = self.files.clone();

        tokio::spawn(async move {
            let _ = job.mark_running("starting", "Export job started");

            let blocking_job = job.clone();
            let blocking_result = tokio::task::spawn_blocking(move || {
                let progress_job = blocking_job.clone();
                let mut report_progress =
                    move |stage: &str, progress_percent: f32, message: &str| {
                        let _ = progress_job.update_progress(stage, progress_percent, message);
                    };

                let cancel_job = blocking_job.clone();
                let is_cancelled = move || cancel_job.is_cancel_requested();

                executor.export_full_data(request, &mut report_progress, &is_cancelled)
            })
            .await;

            match blocking_result {
                Ok(Ok(result)) => {
                    let _ = job.mark_completed_export(result.file_name, result.archive_path);
                }
                Ok(Err(error)) => {
                    if job.is_cancel_requested() || is_cancelled_error(&error) {
                        let _ = job.mark_cancelled();
                    } else {
                        let _ = job.mark_failed(&error.to_string());
                    }

                    let _ = files.cleanup_export(&output_path);
                    let _ = job.clear_export_artifact_path();
                }
                Err(error) => {
                    let _ = job.mark_failed(&format!("Export task join error: {}", error));
                    let _ = files.cleanup_export(&output_path);
                    let _ = job.clear_export_artifact_path();
                }
            }
        });

        Ok(job_id)
    }

    pub fn imports_root(&self) -> PathBuf {
        self.files.imports_root()
    }

    #[cfg(target_os = "ios")]
    pub fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        self.files.prepare_incoming_import_archive_path()
    }

    pub fn get_status(&self, job_id: &str) -> Result<DataArchiveJobStatus, DomainError> {
        self.jobs.get(job_id)?.snapshot()
    }

    #[cfg(target_os = "ios")]
    pub fn completed_export_archive_path(&self, job_id: &str) -> Result<PathBuf, DomainError> {
        Ok(self.completed_export_artifact(job_id)?.archive_path)
    }

    fn completed_export_artifact(
        &self,
        job_id: &str,
    ) -> Result<CompletedExportArtifact, DomainError> {
        let completed = self.completed_export_job(job_id)?;
        let result = &completed.result;
        match result.artifact_state.as_deref() {
            Some(DATA_ARCHIVE_ARTIFACT_DISPOSED) => {
                return Err(DomainError::InvalidData(format!(
                    "Export archive has already been handled for job: {}",
                    job_id
                )));
            }
            Some(DATA_ARCHIVE_ARTIFACT_MISSING) => {
                return Err(DomainError::NotFound(format!(
                    "Export archive is missing for job: {}",
                    job_id
                )));
            }
            Some(DATA_ARCHIVE_ARTIFACT_AVAILABLE) | None => {}
            Some(state) => {
                return Err(DomainError::InvalidData(format!(
                    "Invalid export artifact state for job {}: {}",
                    job_id, state
                )));
            }
        }

        let export_result = result
            .archive_path
            .clone()
            .zip(result.file_name.clone())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Export archive result is missing for job: {}",
                    job_id
                ))
            })?;
        let (archive_path, file_name) = export_result;

        Ok(CompletedExportArtifact {
            job: completed.job,
            archive_path: PathBuf::from(archive_path),
            file_name,
        })
    }

    fn claim_completed_export_artifact(
        &self,
        job_id: &str,
    ) -> Result<CompletedExportArtifact, DomainError> {
        let mut artifact = self.completed_export_artifact(job_id)?;
        artifact.archive_path = artifact.job.claim_export_artifact_path()?.ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Export archive is already being handled for job: {}",
                job_id
            ))
        })?;
        Ok(artifact)
    }

    fn terminal_export_artifact_path(&self, job_id: &str) -> Result<Option<PathBuf>, DomainError> {
        let completed = self.completed_export_job(job_id)?;
        if !matches!(
            completed.result.artifact_state.as_deref(),
            Some(DATA_ARCHIVE_ARTIFACT_DISPOSED | DATA_ARCHIVE_ARTIFACT_MISSING)
        ) {
            return Ok(None);
        }

        let archive_path = completed.result.archive_path.ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Export archive path is missing for job: {}",
                job_id
            ))
        })?;

        Ok(Some(PathBuf::from(archive_path)))
    }

    fn completed_export_job(&self, job_id: &str) -> Result<CompletedExportJob, DomainError> {
        let job = self.jobs.get(job_id)?;
        let status = job.snapshot()?;
        if status.kind != DATA_ARCHIVE_KIND_EXPORT {
            return Err(DomainError::InvalidData("Invalid export job".to_string()));
        }

        if status.state != DATA_ARCHIVE_STATE_COMPLETED {
            return Err(DomainError::InvalidData(format!(
                "Export job is not completed yet: {}",
                job_id
            )));
        }

        let result = status.result.ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Export archive result is missing for job: {}",
                job_id
            ))
        })?;

        Ok(CompletedExportJob { job, result })
    }

    pub fn cancel(&self, job_id: &str) -> Result<(), DomainError> {
        self.jobs.get(job_id)?.request_cancel();
        Ok(())
    }

    pub fn cleanup_export(&self, job_id: &str) -> Result<(), DomainError> {
        let artifact = match self.claim_completed_export_artifact(job_id) {
            Ok(artifact) => artifact,
            Err(error) => {
                if let Some(archive_path) = self.terminal_export_artifact_path(job_id)? {
                    return match self.files.cleanup_export(&archive_path) {
                        Ok(()) | Err(DomainError::NotFound(_)) => Ok(()),
                        Err(error) => Err(error),
                    };
                }
                return Err(error);
            }
        };

        match self.files.cleanup_export(&artifact.archive_path) {
            Ok(()) => artifact.job.mark_export_artifact_disposed(None),
            Err(DomainError::NotFound(_)) => artifact.job.mark_export_artifact_missing(),
            Err(error) => {
                let _ = artifact
                    .job
                    .restore_export_artifact_path(artifact.archive_path);
                Err(error)
            }
        }
    }

    pub async fn save_export(&self, job_id: String) -> Result<PathBuf, DomainError> {
        let artifact = self.claim_completed_export_artifact(&job_id)?;
        let job = artifact.job.clone();
        let archive_path = artifact.archive_path.clone();
        let files = self.files.clone();
        let saved_path = run_blocking("Save export task join error", move || {
            files.save_export(&artifact.archive_path, &artifact.file_name)
        })
        .await;

        match saved_path {
            Ok(saved_path) => {
                job.mark_export_artifact_disposed(Some(saved_path.to_string_lossy().to_string()))?;
                Ok(saved_path)
            }
            Err(error @ DomainError::NotFound(_)) => {
                let _ = job.mark_export_artifact_missing();
                Err(error)
            }
            Err(error) => {
                let _ = job.restore_export_artifact_path(archive_path);
                Err(error)
            }
        }
    }

    pub fn finalize_export_delivery(
        &self,
        job_id: &str,
        saved_target: Option<String>,
    ) -> Result<Option<String>, DomainError> {
        let artifact = match self.claim_completed_export_artifact(job_id) {
            Ok(artifact) => artifact,
            Err(error) => {
                if self.terminal_export_artifact_path(job_id)?.is_some() {
                    return Ok(None);
                }
                return Err(error);
            }
        };

        artifact.job.mark_export_artifact_disposed(saved_target)?;
        match self.files.cleanup_export(&artifact.archive_path) {
            Ok(()) | Err(DomainError::NotFound(_)) => Ok(None),
            Err(error) => Ok(Some(error.to_string())),
        }
    }

    pub async fn export_user_backup(
        &self,
        handle: String,
        include_secrets: bool,
    ) -> Result<UserBackupArchiveResult, DomainError> {
        let executor = self.executor.clone();
        let files = self.files.clone();
        let protected_paths = self.jobs.protected_export_artifact_paths()?;
        run_blocking("User backup export task join error", move || {
            let target =
                files.prepare_user_backup_archive(&handle, include_secrets, &protected_paths)?;
            let output_path = target.request.output_path.clone();
            let mut report_progress = |_stage: &str, _progress_percent: f32, _message: &str| {};
            let is_cancelled = || false;

            if let Err(error) =
                executor.export_user_backup(target.request, &mut report_progress, &is_cancelled)
            {
                let _ = files.cleanup_export(&output_path);
                return Err(error);
            }

            Ok(UserBackupArchiveResult {
                file_name: target.file_name,
                archive_path: output_path.to_string_lossy().to_string(),
            })
        })
        .await
    }

    pub async fn save_user_backup(
        &self,
        archive_path: String,
        file_name: String,
    ) -> Result<PathBuf, DomainError> {
        let files = self.files.clone();
        run_blocking("Save user backup task join error", move || {
            files.save_user_backup(&archive_path, &file_name)
        })
        .await
    }

    pub fn cleanup_user_backup(&self, archive_path: &str) -> Result<(), DomainError> {
        self.files.cleanup_user_backup(archive_path)
    }
}

fn is_cancelled_error(error: &DomainError) -> bool {
    matches!(error, DomainError::Cancelled(_))
}

async fn reconcile_import_data_change(
    data_root_initializer: &Arc<dyn DataRootInitializer>,
    reconciler: &Arc<dyn DataChangeReconciler>,
    data_root: &Path,
) -> Result<(), String> {
    data_root_initializer
        .initialize_data_root(data_root)
        .await
        .map_err(|error| format!("failed to initialize data directory: {}", error))?;
    reconciler
        .reconcile("import")
        .await
        .map_err(|error| format!("failed to refresh runtime caches: {}", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    struct UnusedExecutor;

    impl DataArchiveExecutor for UnusedExecutor {
        fn import_full_data(
            &self,
            _request: ImportArchiveExecutionRequest,
            _report_progress: &mut dyn FnMut(&str, f32, &str),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<ArchiveImportExecutionReport, DataArchiveImportFailure> {
            unreachable!()
        }

        fn export_full_data(
            &self,
            _request: ExportArchiveExecutionRequest,
            _report_progress: &mut dyn FnMut(&str, f32, &str),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<ArchiveExportExecutionReport, DomainError> {
            unreachable!()
        }

        fn export_user_backup(
            &self,
            _request: UserBackupArchiveExecutionRequest,
            _report_progress: &mut dyn FnMut(&str, f32, &str),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<(), DomainError> {
            unreachable!()
        }
    }

    struct UnusedFiles;

    impl DataArchiveFileGateway for UnusedFiles {
        fn imports_root(&self) -> PathBuf {
            unreachable!()
        }

        #[cfg(target_os = "ios")]
        fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
            unreachable!()
        }

        fn prepare_import_archive(
            &self,
            _archive_path: &Path,
            _archive_is_temporary: bool,
            _job_id: &str,
        ) -> Result<ImportArchiveExecutionRequest, DomainError> {
            unreachable!()
        }

        fn prepare_export_archive(
            &self,
            _job_id: &str,
            _protected_paths: &[PathBuf],
        ) -> Result<ExportArchiveExecutionRequest, DomainError> {
            unreachable!()
        }

        fn prepare_user_backup_archive(
            &self,
            _handle: &str,
            _include_secrets: bool,
            _protected_paths: &[PathBuf],
        ) -> Result<UserBackupArchiveTarget, DomainError> {
            unreachable!()
        }

        fn cleanup_directory(&self, _path: &Path) {
            unreachable!()
        }

        fn cleanup_export(&self, _archive_path: &Path) -> Result<(), DomainError> {
            unreachable!()
        }

        fn save_export(
            &self,
            _archive_path: &Path,
            _file_name: &str,
        ) -> Result<PathBuf, DomainError> {
            unreachable!()
        }

        fn save_user_backup(
            &self,
            _archive_path: &str,
            _file_name: &str,
        ) -> Result<PathBuf, DomainError> {
            unreachable!()
        }

        fn cleanup_user_backup(&self, _archive_path: &str) -> Result<(), DomainError> {
            unreachable!()
        }
    }

    struct UnusedInitializer;

    #[async_trait]
    impl DataRootInitializer for UnusedInitializer {
        async fn initialize_data_root(&self, _data_root: &Path) -> Result<(), DomainError> {
            unreachable!()
        }
    }

    struct UnusedReconciler;

    #[async_trait]
    impl DataChangeReconciler for UnusedReconciler {
        async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct RecordingExecutor {
        import_result:
            Mutex<Option<Result<ArchiveImportExecutionReport, DataArchiveImportFailure>>>,
        export_result: Mutex<Option<Result<String, DomainError>>>,
    }

    impl RecordingExecutor {
        fn import_ok(source_users: Vec<String>, target_user: &str) -> Self {
            Self {
                import_result: Mutex::new(Some(Ok(ArchiveImportExecutionReport {
                    source_users,
                    target_user: target_user.to_string(),
                    local_applied: import_local_applied(),
                }))),
                ..Self::default()
            }
        }

        fn import_ok_without_local_mutation(source_users: Vec<String>, target_user: &str) -> Self {
            Self {
                import_result: Mutex::new(Some(Ok(ArchiveImportExecutionReport {
                    source_users,
                    target_user: target_user.to_string(),
                    local_applied: DataArchiveLocalMutationSummary::default(),
                }))),
                ..Self::default()
            }
        }

        fn import_error(
            error: DomainError,
            local_applied: DataArchiveLocalMutationSummary,
        ) -> Self {
            Self {
                import_result: Mutex::new(Some(Err(DataArchiveImportFailure::new(
                    error,
                    local_applied,
                )))),
                ..Self::default()
            }
        }

        fn export_ok(file_name: &str) -> Self {
            Self {
                export_result: Mutex::new(Some(Ok(file_name.to_string()))),
                ..Self::default()
            }
        }

        fn export_error(error: DomainError) -> Self {
            Self {
                export_result: Mutex::new(Some(Err(error))),
                ..Self::default()
            }
        }
    }

    impl DataArchiveExecutor for RecordingExecutor {
        fn import_full_data(
            &self,
            _request: ImportArchiveExecutionRequest,
            _report_progress: &mut dyn FnMut(&str, f32, &str),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<ArchiveImportExecutionReport, DataArchiveImportFailure> {
            self.import_result
                .lock()
                .expect("lock import result")
                .take()
                .unwrap_or_else(|| {
                    Err(DataArchiveImportFailure::new(
                        DomainError::InternalError("missing import result".to_string()),
                        DataArchiveLocalMutationSummary::default(),
                    ))
                })
        }

        fn export_full_data(
            &self,
            request: ExportArchiveExecutionRequest,
            _report_progress: &mut dyn FnMut(&str, f32, &str),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<ArchiveExportExecutionReport, DomainError> {
            match self
                .export_result
                .lock()
                .expect("lock export result")
                .take()
                .ok_or_else(|| DomainError::InternalError("missing export result".to_string()))?
            {
                Ok(file_name) => Ok(ArchiveExportExecutionReport {
                    file_name,
                    archive_path: request.output_path,
                }),
                Err(error) => Err(error),
            }
        }

        fn export_user_backup(
            &self,
            _request: UserBackupArchiveExecutionRequest,
            _report_progress: &mut dyn FnMut(&str, f32, &str),
            _is_cancelled: &dyn Fn() -> bool,
        ) -> Result<(), DomainError> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct RecordingFiles {
        import_request: Mutex<Option<ImportArchiveExecutionRequest>>,
        export_request: Mutex<Option<ExportArchiveExecutionRequest>>,
        export_protected_paths: Mutex<Vec<Vec<PathBuf>>>,
        save_export_result: Mutex<Option<Result<PathBuf, DomainError>>>,
        cleanup_export_result: Mutex<Option<Result<(), DomainError>>>,
        cleaned_directories: Mutex<Vec<PathBuf>>,
        cleaned_exports: Mutex<Vec<PathBuf>>,
    }

    impl RecordingFiles {
        fn with_import(request: ImportArchiveExecutionRequest) -> Self {
            Self {
                import_request: Mutex::new(Some(request)),
                ..Self::default()
            }
        }

        fn with_export(request: ExportArchiveExecutionRequest) -> Self {
            Self {
                export_request: Mutex::new(Some(request)),
                ..Self::default()
            }
        }

        fn with_save_export(saved_path: PathBuf) -> Self {
            Self {
                save_export_result: Mutex::new(Some(Ok(saved_path))),
                ..Self::default()
            }
        }

        fn with_save_export_result(result: Result<PathBuf, DomainError>) -> Self {
            Self {
                save_export_result: Mutex::new(Some(result)),
                ..Self::default()
            }
        }

        fn with_cleanup_export_result(result: Result<(), DomainError>) -> Self {
            Self {
                cleanup_export_result: Mutex::new(Some(result)),
                ..Self::default()
            }
        }
    }

    impl DataArchiveFileGateway for RecordingFiles {
        fn imports_root(&self) -> PathBuf {
            PathBuf::from("/tmp/imports")
        }

        #[cfg(target_os = "ios")]
        fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
            unreachable!()
        }

        fn prepare_import_archive(
            &self,
            _archive_path: &Path,
            _archive_is_temporary: bool,
            _job_id: &str,
        ) -> Result<ImportArchiveExecutionRequest, DomainError> {
            self.import_request
                .lock()
                .expect("lock import request")
                .take()
                .ok_or_else(|| DomainError::InternalError("missing import request".to_string()))
        }

        fn prepare_export_archive(
            &self,
            _job_id: &str,
            protected_paths: &[PathBuf],
        ) -> Result<ExportArchiveExecutionRequest, DomainError> {
            self.export_protected_paths
                .lock()
                .expect("lock export protected paths")
                .push(protected_paths.to_vec());
            self.export_request
                .lock()
                .expect("lock export request")
                .take()
                .ok_or_else(|| DomainError::InternalError("missing export request".to_string()))
        }

        fn prepare_user_backup_archive(
            &self,
            _handle: &str,
            _include_secrets: bool,
            _protected_paths: &[PathBuf],
        ) -> Result<UserBackupArchiveTarget, DomainError> {
            unreachable!()
        }

        fn cleanup_directory(&self, path: &Path) {
            self.cleaned_directories
                .lock()
                .expect("lock cleaned directories")
                .push(path.to_path_buf());
        }

        fn cleanup_export(&self, archive_path: &Path) -> Result<(), DomainError> {
            self.cleaned_exports
                .lock()
                .expect("lock cleaned exports")
                .push(archive_path.to_path_buf());
            self.cleanup_export_result
                .lock()
                .expect("lock cleanup export result")
                .take()
                .unwrap_or(Ok(()))
        }

        fn save_export(
            &self,
            _archive_path: &Path,
            _file_name: &str,
        ) -> Result<PathBuf, DomainError> {
            self.save_export_result
                .lock()
                .expect("lock save export result")
                .take()
                .unwrap_or_else(|| {
                    Err(DomainError::InternalError(
                        "missing save export result".to_string(),
                    ))
                })
        }

        fn save_user_backup(
            &self,
            _archive_path: &str,
            _file_name: &str,
        ) -> Result<PathBuf, DomainError> {
            unreachable!()
        }

        fn cleanup_user_backup(&self, _archive_path: &str) -> Result<(), DomainError> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct RecordingInitializer {
        data_roots: Mutex<Vec<PathBuf>>,
    }

    #[async_trait]
    impl DataRootInitializer for RecordingInitializer {
        async fn initialize_data_root(&self, data_root: &Path) -> Result<(), DomainError> {
            self.data_roots
                .lock()
                .expect("lock initialized data roots")
                .push(data_root.to_path_buf());
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingReconciler {
        reasons: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl DataChangeReconciler for RecordingReconciler {
        async fn reconcile(&self, reason: &str) -> Result<(), DomainError> {
            self.reasons
                .lock()
                .expect("lock reconcile reasons")
                .push(reason.to_string());
            Ok(())
        }
    }

    struct FailingReconciler;

    #[async_trait]
    impl DataChangeReconciler for FailingReconciler {
        async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
            Err(DomainError::InternalError("cache stale".to_string()))
        }
    }

    fn import_local_applied() -> DataArchiveLocalMutationSummary {
        DataArchiveLocalMutationSummary {
            files_written: 1,
            bytes_written: 7,
            target_changed: true,
        }
    }

    async fn wait_for_job_state(
        service: &DataArchiveService,
        job_id: &str,
        expected_state: &str,
    ) -> DataArchiveJobStatus {
        for _ in 0..100 {
            let status = service.get_status(job_id).expect("job status");
            if status.state == expected_state {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        panic!("job {job_id} did not reach state {expected_state}");
    }

    async fn wait_until(mut predicate: impl FnMut() -> bool) {
        for _ in 0..100 {
            if predicate() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        panic!("condition was not met");
    }

    #[test]
    fn registries_are_instance_scoped() {
        let job = Arc::new(DataArchiveJobHandle::new("job-1", "export"));
        let first = DataArchiveJobRegistry::new();
        let second = DataArchiveJobRegistry::new();

        first.insert("job-1", job).expect("insert job");

        assert!(first.get("job-1").is_ok());
        assert!(second.get("job-1").is_err());
    }

    #[test]
    fn dropping_registry_requests_job_cancellation() {
        let job = Arc::new(DataArchiveJobHandle::new("job-1", "import"));
        let registry = DataArchiveJobRegistry::new();

        registry.insert("job-1", job.clone()).expect("insert job");
        drop(registry);

        assert!(job.is_cancel_requested());
    }

    #[test]
    fn service_resolves_completed_export_artifact() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
        job.mark_completed_export(
            "tauritavern-data.zip".to_string(),
            PathBuf::from("/tmp/tauritavern-data.zip"),
        )
        .expect("mark completed export");
        jobs.insert("job-1", job).expect("insert job");

        let service = DataArchiveService::new(
            jobs,
            Arc::new(UnusedExecutor),
            Arc::new(UnusedFiles),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        let artifact = service
            .completed_export_artifact("job-1")
            .expect("completed export artifact");
        assert_eq!(
            artifact.archive_path,
            PathBuf::from("/tmp/tauritavern-data.zip")
        );
        assert_eq!(artifact.file_name, "tauritavern-data.zip");
    }

    #[tokio::test]
    async fn start_import_runs_executor_initializer_reconciler_and_cleanup() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let data_root = PathBuf::from("/tmp/tauritavern-data-root");
        let workspace_root = PathBuf::from("/tmp/tauritavern-import-workspace");
        let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
            data_root: data_root.clone(),
            archive_path: PathBuf::from("/tmp/import.archive"),
            workspace_root: workspace_root.clone(),
        }));
        let initializer = Arc::new(RecordingInitializer::default());
        let reconciler = Arc::new(RecordingReconciler::default());
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::import_ok(
                vec!["alice".to_string()],
                "alice",
            )),
            files.clone(),
            initializer.clone(),
            reconciler.clone(),
        );

        let job_id = service
            .start_import(Path::new("/tmp/source.archive"), true)
            .expect("start import");

        let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
        let result = status.result.expect("completed import result");
        assert_eq!(result.source_users, vec!["alice"]);
        assert_eq!(result.target_user.as_deref(), Some("alice"));

        wait_until(|| {
            files
                .cleaned_directories
                .lock()
                .expect("lock cleaned directories")
                .contains(&workspace_root)
        })
        .await;
        assert_eq!(
            *initializer
                .data_roots
                .lock()
                .expect("lock initialized data roots"),
            vec![data_root]
        );
        assert_eq!(
            *reconciler.reasons.lock().expect("lock reconcile reasons"),
            vec!["import".to_string()]
        );
    }

    #[tokio::test]
    async fn start_import_with_no_local_mutation_skips_initializer_and_reconciler() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let data_root = PathBuf::from("/tmp/tauritavern-noop-data-root");
        let workspace_root = PathBuf::from("/tmp/tauritavern-noop-import-workspace");
        let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
            data_root,
            archive_path: PathBuf::from("/tmp/import.archive"),
            workspace_root,
        }));
        let initializer = Arc::new(RecordingInitializer::default());
        let reconciler = Arc::new(RecordingReconciler::default());
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::import_ok_without_local_mutation(
                vec!["alice".to_string()],
                "alice",
            )),
            files,
            initializer.clone(),
            reconciler.clone(),
        );

        let job_id = service
            .start_import(Path::new("/tmp/source.archive"), true)
            .expect("start import");

        let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
        assert_eq!(status.local_applied, None);
        assert!(
            initializer
                .data_roots
                .lock()
                .expect("lock initialized data roots")
                .is_empty()
        );
        assert!(
            reconciler
                .reasons
                .lock()
                .expect("lock reconcile reasons")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn partial_import_failure_initializes_reconciles_and_reports_local_mutation() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let data_root = PathBuf::from("/tmp/tauritavern-partial-data-root");
        let workspace_root = PathBuf::from("/tmp/tauritavern-partial-import-workspace");
        let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
            data_root: data_root.clone(),
            archive_path: PathBuf::from("/tmp/import.archive"),
            workspace_root,
        }));
        let initializer = Arc::new(RecordingInitializer::default());
        let reconciler = Arc::new(RecordingReconciler::default());
        let local_applied = import_local_applied();
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::import_error(
                DomainError::InternalError("boom".to_string()),
                local_applied,
            )),
            files,
            initializer.clone(),
            reconciler.clone(),
        );

        let job_id = service
            .start_import(Path::new("/tmp/source.archive"), true)
            .expect("start import");

        let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
        assert_eq!(status.error.as_deref(), Some("Internal error: boom"));
        assert_eq!(status.local_applied, Some(local_applied.into()));
        assert_eq!(status.reconcile_error, None);
        assert_eq!(
            *initializer
                .data_roots
                .lock()
                .expect("lock initialized data roots"),
            vec![data_root]
        );
        assert_eq!(
            *reconciler.reasons.lock().expect("lock reconcile reasons"),
            vec!["import".to_string()]
        );
    }

    #[tokio::test]
    async fn partial_import_cancel_initializes_reconciles_and_reports_local_mutation() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let data_root = PathBuf::from("/tmp/tauritavern-partial-cancel-data-root");
        let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
            data_root: data_root.clone(),
            archive_path: PathBuf::from("/tmp/import.archive"),
            workspace_root: PathBuf::from("/tmp/tauritavern-partial-cancel-workspace"),
        }));
        let initializer = Arc::new(RecordingInitializer::default());
        let reconciler = Arc::new(RecordingReconciler::default());
        let local_applied = import_local_applied();
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::import_error(
                DomainError::Cancelled("cancelled".to_string()),
                local_applied,
            )),
            files,
            initializer.clone(),
            reconciler.clone(),
        );

        let job_id = service
            .start_import(Path::new("/tmp/source.archive"), true)
            .expect("start import");

        let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_CANCELLED).await;
        assert_eq!(status.local_applied, Some(local_applied.into()));
        assert_eq!(
            *initializer
                .data_roots
                .lock()
                .expect("lock initialized data roots"),
            vec![data_root]
        );
        assert_eq!(
            *reconciler.reasons.lock().expect("lock reconcile reasons"),
            vec!["import".to_string()]
        );
    }

    #[tokio::test]
    async fn partial_import_failure_reports_reconcile_error() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let data_root = PathBuf::from("/tmp/tauritavern-partial-reconcile-data-root");
        let files = Arc::new(RecordingFiles::with_import(ImportArchiveExecutionRequest {
            data_root,
            archive_path: PathBuf::from("/tmp/import.archive"),
            workspace_root: PathBuf::from("/tmp/tauritavern-partial-reconcile-workspace"),
        }));
        let local_applied = import_local_applied();
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::import_error(
                DomainError::InternalError("boom".to_string()),
                local_applied,
            )),
            files,
            Arc::new(RecordingInitializer::default()),
            Arc::new(FailingReconciler),
        );

        let job_id = service
            .start_import(Path::new("/tmp/source.archive"), true)
            .expect("start import");

        let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
        assert_eq!(status.local_applied, Some(local_applied.into()));
        assert_eq!(
            status.reconcile_error.as_deref(),
            Some("failed to refresh runtime caches: Internal error: cache stale")
        );
    }

    #[tokio::test]
    async fn start_export_runs_executor_and_marks_completed() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let output_path = PathBuf::from("/tmp/tauritavern-data.zip");
        let files = Arc::new(RecordingFiles::with_export(ExportArchiveExecutionRequest {
            data_root: PathBuf::from("/tmp/data-root"),
            output_path: output_path.clone(),
            file_name: "tauritavern-data.zip".to_string(),
        }));
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::export_ok("tauritavern-data.zip")),
            files,
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        let job_id = service.start_export().expect("start export");

        let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_COMPLETED).await;
        let result = status.result.expect("completed export result");
        assert_eq!(result.file_name.as_deref(), Some("tauritavern-data.zip"));
        assert_eq!(
            result.archive_path.as_deref(),
            Some(output_path.to_string_lossy().as_ref())
        );
        assert_eq!(
            result.artifact_state.as_deref(),
            Some(DATA_ARCHIVE_ARTIFACT_AVAILABLE)
        );
    }

    #[tokio::test]
    async fn start_export_cleans_partial_archive_on_failure() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let output_path = PathBuf::from("/tmp/partial-tauritavern-data.zip");
        let files = Arc::new(RecordingFiles::with_export(ExportArchiveExecutionRequest {
            data_root: PathBuf::from("/tmp/data-root"),
            output_path: output_path.clone(),
            file_name: "tauritavern-data.zip".to_string(),
        }));
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::export_error(DomainError::InternalError(
                "boom".to_string(),
            ))),
            files.clone(),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        let job_id = service.start_export().expect("start export");

        let status = wait_for_job_state(&service, &job_id, DATA_ARCHIVE_STATE_FAILED).await;
        assert_eq!(status.error.as_deref(), Some("Internal error: boom"));
        wait_until(|| {
            files
                .cleaned_exports
                .lock()
                .expect("lock cleaned exports")
                .contains(&output_path)
        })
        .await;
    }

    #[tokio::test]
    async fn start_export_protects_claimed_completed_artifact_from_stale_cleanup() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let existing_job = Arc::new(DataArchiveJobHandle::new(
            "old-job",
            DATA_ARCHIVE_KIND_EXPORT,
        ));
        let existing_path = PathBuf::from("/tmp/old-staged-export.zip");
        existing_job
            .mark_completed_export("tauritavern-data.zip".to_string(), existing_path.clone())
            .expect("mark completed export");
        existing_job
            .claim_export_artifact_path()
            .expect("claim artifact path");
        jobs.insert("old-job", existing_job)
            .expect("insert old job");

        let files = Arc::new(RecordingFiles::with_export(ExportArchiveExecutionRequest {
            data_root: PathBuf::from("/tmp/data-root"),
            output_path: PathBuf::from("/tmp/new-staged-export.zip"),
            file_name: "tauritavern-data.zip".to_string(),
        }));
        let service = DataArchiveService::new(
            jobs,
            Arc::new(RecordingExecutor::export_ok("tauritavern-data.zip")),
            files.clone(),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        let _ = service.start_export().expect("start export");

        assert_eq!(
            *files
                .export_protected_paths
                .lock()
                .expect("lock export protected paths"),
            vec![vec![existing_path]]
        );
    }

    #[tokio::test]
    async fn save_export_marks_artifact_disposed_with_saved_path() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
        job.mark_completed_export(
            "tauritavern-data.zip".to_string(),
            PathBuf::from("/tmp/staged-export.zip"),
        )
        .expect("mark completed export");
        jobs.insert("job-1", job).expect("insert job");

        let saved_path = PathBuf::from("/Downloads/tauritavern-data.zip");
        let service = DataArchiveService::new(
            jobs,
            Arc::new(UnusedExecutor),
            Arc::new(RecordingFiles::with_save_export(saved_path.clone())),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        assert_eq!(
            service
                .save_export("job-1".to_string())
                .await
                .expect("save export"),
            saved_path
        );

        let status = service.get_status("job-1").expect("job status");
        let result = status.result.expect("export result");
        assert_eq!(
            result.artifact_state.as_deref(),
            Some(DATA_ARCHIVE_ARTIFACT_DISPOSED)
        );
        assert_eq!(
            result.archive_path.as_deref(),
            Some("/tmp/staged-export.zip")
        );
        assert_eq!(
            result.saved_path.as_deref(),
            Some("/Downloads/tauritavern-data.zip")
        );
        let error = match service.completed_export_artifact("job-1") {
            Ok(_) => panic!("disposed artifact should not be reusable"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("already been handled"));
    }

    #[test]
    fn cleanup_export_marks_artifact_disposed_and_is_idempotent() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
        let archive_path = PathBuf::from("/tmp/staged-export.zip");
        job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
            .expect("mark completed export");
        jobs.insert("job-1", job).expect("insert job");

        let files = Arc::new(RecordingFiles::default());
        let service = DataArchiveService::new(
            jobs,
            Arc::new(UnusedExecutor),
            files.clone(),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        service.cleanup_export("job-1").expect("cleanup export");
        service
            .cleanup_export("job-1")
            .expect("cleanup export is idempotent");

        assert_eq!(
            *files.cleaned_exports.lock().expect("lock cleaned exports"),
            vec![archive_path.clone(), archive_path]
        );
        let status = service.get_status("job-1").expect("job status");
        let result = status.result.expect("export result");
        assert_eq!(
            result.artifact_state.as_deref(),
            Some(DATA_ARCHIVE_ARTIFACT_DISPOSED)
        );
        assert_eq!(
            result.archive_path.as_deref(),
            Some("/tmp/staged-export.zip")
        );
    }

    #[test]
    fn cleanup_export_marks_missing_when_artifact_is_already_gone() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
        let archive_path = PathBuf::from("/tmp/staged-export.zip");
        job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
            .expect("mark completed export");
        jobs.insert("job-1", job).expect("insert job");

        let files = Arc::new(RecordingFiles::with_cleanup_export_result(Err(
            DomainError::NotFound("gone".to_string()),
        )));
        let service = DataArchiveService::new(
            jobs,
            Arc::new(UnusedExecutor),
            files.clone(),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        service.cleanup_export("job-1").expect("cleanup export");
        service
            .cleanup_export("job-1")
            .expect("missing cleanup is idempotent");

        assert_eq!(
            *files.cleaned_exports.lock().expect("lock cleaned exports"),
            vec![archive_path.clone(), archive_path]
        );
        let result = service
            .get_status("job-1")
            .expect("job status")
            .result
            .expect("export result");
        assert_eq!(
            result.artifact_state.as_deref(),
            Some(DATA_ARCHIVE_ARTIFACT_MISSING)
        );
        assert_eq!(
            result.archive_path.as_deref(),
            Some("/tmp/staged-export.zip")
        );
    }

    #[tokio::test]
    async fn save_export_restores_available_artifact_on_save_error() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
        let archive_path = PathBuf::from("/tmp/staged-export.zip");
        job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
            .expect("mark completed export");
        jobs.insert("job-1", job).expect("insert job");

        let files = Arc::new(RecordingFiles::with_save_export_result(Err(
            DomainError::InvalidData("target exists".to_string()),
        )));
        let service = DataArchiveService::new(
            jobs,
            Arc::new(UnusedExecutor),
            files.clone(),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        assert!(
            service
                .save_export("job-1".to_string())
                .await
                .expect_err("save should fail")
                .to_string()
                .contains("target exists")
        );
        service.cleanup_export("job-1").expect("cleanup can retry");

        assert_eq!(
            *files.cleaned_exports.lock().expect("lock cleaned exports"),
            vec![archive_path]
        );
    }

    #[test]
    fn finalize_export_delivery_disposes_even_when_cleanup_fails() {
        let jobs = Arc::new(DataArchiveJobRegistry::new());
        let job = Arc::new(DataArchiveJobHandle::new("job-1", DATA_ARCHIVE_KIND_EXPORT));
        let archive_path = PathBuf::from("/tmp/staged-export.zip");
        job.mark_completed_export("tauritavern-data.zip".to_string(), archive_path.clone())
            .expect("mark completed export");
        jobs.insert("job-1", job).expect("insert job");

        let files = Arc::new(RecordingFiles::with_cleanup_export_result(Err(
            DomainError::InternalError("permission denied".to_string()),
        )));
        let service = DataArchiveService::new(
            jobs,
            Arc::new(UnusedExecutor),
            files.clone(),
            Arc::new(UnusedInitializer),
            Arc::new(UnusedReconciler),
        );

        assert!(
            service
                .finalize_export_delivery("job-1", Some("content://saved-export".to_string()))
                .expect("finalize delivery")
                .expect("cleanup warning")
                .contains("permission denied")
        );
        service
            .cleanup_export("job-1")
            .expect("disposed cleanup can retry deletion");

        assert_eq!(
            *files.cleaned_exports.lock().expect("lock cleaned exports"),
            vec![archive_path.clone(), archive_path]
        );
        let result = service
            .get_status("job-1")
            .expect("job status")
            .result
            .expect("export result");
        assert_eq!(
            result.artifact_state.as_deref(),
            Some(DATA_ARCHIVE_ARTIFACT_DISPOSED)
        );
        assert_eq!(
            result.archive_path.as_deref(),
            Some("/tmp/staged-export.zip")
        );
        assert_eq!(result.saved_path.as_deref(), Some("content://saved-export"));
    }
}

async fn run_blocking<T>(
    context: &'static str,
    operation: impl FnOnce() -> Result<T, DomainError> + Send + 'static,
) -> Result<T, DomainError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| DomainError::InternalError(format!("{}: {}", context, error)))?
}
