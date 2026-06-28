use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::application::dto::data_archive_dto::{
    DATA_ARCHIVE_ARTIFACT_AVAILABLE, DATA_ARCHIVE_ARTIFACT_DISPOSED, DATA_ARCHIVE_ARTIFACT_MISSING,
    DATA_ARCHIVE_KIND_EXPORT, DATA_ARCHIVE_STATE_CANCELLED, DATA_ARCHIVE_STATE_COMPLETED,
    DATA_ARCHIVE_STATE_FAILED, DATA_ARCHIVE_STATE_PENDING, DATA_ARCHIVE_STATE_RUNNING,
    DataArchiveJobResult, DataArchiveJobStatus,
};
use crate::domain::errors::DomainError;
use crate::domain::models::data_archive::DataArchiveLocalMutationSummary;

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

    pub(super) fn claim_export_artifact_path(&self) -> Result<Option<PathBuf>, DomainError> {
        let mut path = self.export_artifact_path.lock().map_err(|_| {
            DomainError::InternalError("Failed to lock export artifact path".to_string())
        })?;
        Ok(path.take())
    }

    pub(super) fn restore_export_artifact_path(
        &self,
        artifact_path: PathBuf,
    ) -> Result<(), DomainError> {
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
