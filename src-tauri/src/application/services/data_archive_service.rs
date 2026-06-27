use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::application::dto::data_archive_dto::DATA_ARCHIVE_KIND_EXPORT;
use crate::application::dto::data_archive_dto::{
    DATA_ARCHIVE_STATE_CANCELLED, DATA_ARCHIVE_STATE_COMPLETED, DATA_ARCHIVE_STATE_FAILED,
    DATA_ARCHIVE_STATE_PENDING, DATA_ARCHIVE_STATE_RUNNING, DataArchiveJobResult,
    DataArchiveJobStatus, UserBackupArchiveResult,
};
use crate::domain::errors::DomainError;

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
                started_at: Utc::now().to_rfc3339(),
                finished_at: None,
            }),
            cancel_requested: AtomicBool::new(false),
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
            });
            status.error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_completed_export(
        &self,
        file_name: String,
        archive_path: PathBuf,
    ) -> Result<(), DomainError> {
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
            });
            status.error = None;
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_failed(&self, error_message: &str) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_FAILED.to_string();
            status.stage = "failed".to_string();
            status.message = "Job failed".to_string();
            status.error = Some(error_message.to_string());
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub(crate) fn mark_cancelled(&self) -> Result<(), DomainError> {
        self.update_status(|status| {
            status.state = DATA_ARCHIVE_STATE_CANCELLED.to_string();
            status.stage = "cancelled".to_string();
            status.message = "Job cancelled".to_string();
            status.finished_at = Some(Utc::now().to_rfc3339());
        })
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

pub(crate) trait DataArchiveJobBackend: Send + Sync {
    fn start_import(
        &self,
        archive_path: &Path,
        archive_is_temporary: bool,
    ) -> Result<String, DomainError>;
    fn start_export(&self) -> Result<String, DomainError>;
    fn imports_root(&self) -> PathBuf;
    #[cfg(target_os = "ios")]
    fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError>;
    fn cleanup_export(&self, archive_path: &Path) -> Result<(), DomainError>;
    fn save_export(&self, archive_path: &Path, file_name: &str) -> Result<PathBuf, DomainError>;
    fn export_user_backup(
        &self,
        handle: &str,
        include_secrets: bool,
    ) -> Result<UserBackupArchiveResult, DomainError>;
    fn save_user_backup(&self, archive_path: &str, file_name: &str)
    -> Result<PathBuf, DomainError>;
    fn cleanup_user_backup(&self, archive_path: &str) -> Result<(), DomainError>;
}

pub struct DataArchiveService {
    jobs: Arc<DataArchiveJobRegistry>,
    backend: Arc<dyn DataArchiveJobBackend>,
}

impl DataArchiveService {
    pub(crate) fn new(
        jobs: Arc<DataArchiveJobRegistry>,
        backend: Arc<dyn DataArchiveJobBackend>,
    ) -> Self {
        Self { jobs, backend }
    }

    pub fn start_import(
        &self,
        archive_path: &Path,
        archive_is_temporary: bool,
    ) -> Result<String, DomainError> {
        self.backend
            .start_import(archive_path, archive_is_temporary)
    }

    pub fn start_export(&self) -> Result<String, DomainError> {
        self.backend.start_export()
    }

    pub fn imports_root(&self) -> PathBuf {
        self.backend.imports_root()
    }

    #[cfg(target_os = "ios")]
    pub fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
        self.backend.prepare_incoming_import_archive_path()
    }

    pub fn get_status(&self, job_id: &str) -> Result<DataArchiveJobStatus, DomainError> {
        self.jobs.get(job_id)?.snapshot()
    }

    pub fn completed_export_archive_path(&self, job_id: &str) -> Result<PathBuf, DomainError> {
        let result = self.completed_export_result(job_id)?;
        let archive_path = result.archive_path.ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Export archive path is missing for job: {}",
                job_id
            ))
        })?;

        Ok(PathBuf::from(archive_path))
    }

    fn completed_export_artifact(&self, job_id: &str) -> Result<(PathBuf, String), DomainError> {
        let result = self.completed_export_result(job_id)?;
        let (archive_path, file_name) =
            result.archive_path.zip(result.file_name).ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Export archive result is missing for job: {}",
                    job_id
                ))
            })?;

        Ok((PathBuf::from(archive_path), file_name))
    }

    fn completed_export_result(&self, job_id: &str) -> Result<DataArchiveJobResult, DomainError> {
        let status = self.get_status(job_id)?;
        if status.kind != DATA_ARCHIVE_KIND_EXPORT {
            return Err(DomainError::InvalidData("Invalid export job".to_string()));
        }

        if status.state != DATA_ARCHIVE_STATE_COMPLETED {
            return Err(DomainError::InvalidData(format!(
                "Export job is not completed yet: {}",
                job_id
            )));
        }

        status.result.ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Export archive result is missing for job: {}",
                job_id
            ))
        })
    }

    pub fn cancel(&self, job_id: &str) -> Result<(), DomainError> {
        self.jobs.get(job_id)?.request_cancel();
        Ok(())
    }

    pub fn cleanup_export(&self, job_id: &str) -> Result<(), DomainError> {
        let archive_path = self.completed_export_archive_path(job_id)?;
        self.backend.cleanup_export(&archive_path)
    }

    pub async fn save_export(&self, job_id: String) -> Result<PathBuf, DomainError> {
        let (archive_path, file_name) = self.completed_export_artifact(&job_id)?;
        let backend = self.backend.clone();
        run_blocking("Save export task join error", move || {
            backend.save_export(&archive_path, &file_name)
        })
        .await
    }

    pub async fn export_user_backup(
        &self,
        handle: String,
        include_secrets: bool,
    ) -> Result<UserBackupArchiveResult, DomainError> {
        let backend = self.backend.clone();
        run_blocking("User backup export task join error", move || {
            backend.export_user_backup(&handle, include_secrets)
        })
        .await
    }

    pub async fn save_user_backup(
        &self,
        archive_path: String,
        file_name: String,
    ) -> Result<PathBuf, DomainError> {
        let backend = self.backend.clone();
        run_blocking("Save user backup task join error", move || {
            backend.save_user_backup(&archive_path, &file_name)
        })
        .await
    }

    pub fn cleanup_user_backup(&self, archive_path: &str) -> Result<(), DomainError> {
        self.backend.cleanup_user_backup(archive_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UnusedBackend;

    impl DataArchiveJobBackend for UnusedBackend {
        fn start_import(
            &self,
            _archive_path: &Path,
            _archive_is_temporary: bool,
        ) -> Result<String, DomainError> {
            unreachable!()
        }

        fn start_export(&self) -> Result<String, DomainError> {
            unreachable!()
        }

        fn imports_root(&self) -> PathBuf {
            unreachable!()
        }

        #[cfg(target_os = "ios")]
        fn prepare_incoming_import_archive_path(&self) -> Result<PathBuf, DomainError> {
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

        fn export_user_backup(
            &self,
            _handle: &str,
            _include_secrets: bool,
        ) -> Result<UserBackupArchiveResult, DomainError> {
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

        let service = DataArchiveService::new(jobs, Arc::new(UnusedBackend));

        assert_eq!(
            service
                .completed_export_artifact("job-1")
                .expect("completed export artifact"),
            (
                PathBuf::from("/tmp/tauritavern-data.zip"),
                "tauritavern-data.zip".to_string()
            )
        );
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
