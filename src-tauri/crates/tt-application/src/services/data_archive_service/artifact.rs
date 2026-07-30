use std::path::PathBuf;
use std::sync::Arc;

use crate::dto::data_archive_dto::{
    DATA_ARCHIVE_ARTIFACT_AVAILABLE, DATA_ARCHIVE_ARTIFACT_DISPOSED, DATA_ARCHIVE_ARTIFACT_MISSING,
    DATA_ARCHIVE_KIND_EXPORT, DATA_ARCHIVE_STATE_COMPLETED, DataArchiveJobResult,
    UserBackupArchiveResult,
};
use tt_domain::errors::DomainError;

use super::{DataArchiveJobHandle, DataArchiveService, run_blocking};

struct CompletedExportJob {
    pub(super) job: Arc<DataArchiveJobHandle>,
    result: DataArchiveJobResult,
}

pub(super) struct CompletedExportArtifact {
    pub(super) job: Arc<DataArchiveJobHandle>,
    pub(super) archive_path: PathBuf,
    pub(super) file_name: String,
}

impl DataArchiveService {
    #[cfg(target_os = "ios")]
    pub fn completed_export_archive_path(&self, job_id: &str) -> Result<PathBuf, DomainError> {
        Ok(self.completed_export_artifact(job_id)?.archive_path)
    }

    pub(super) fn completed_export_artifact(
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
        let saved_path = run_blocking(
            self.runtime.clone(),
            "Save export task join error",
            move || files.save_export(&artifact.archive_path, &artifact.file_name),
        )
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
        run_blocking(
            self.runtime.clone(),
            "User backup export task join error",
            move || {
                let target = files.prepare_user_backup_archive(
                    &handle,
                    include_secrets,
                    &protected_paths,
                )?;
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
            },
        )
        .await
    }

    pub async fn save_user_backup(
        &self,
        archive_path: String,
        file_name: String,
    ) -> Result<PathBuf, DomainError> {
        let files = self.files.clone();
        run_blocking(
            self.runtime.clone(),
            "Save user backup task join error",
            move || files.save_user_backup(&archive_path, &file_name),
        )
        .await
    }

    pub fn cleanup_user_backup(&self, archive_path: &str) -> Result<(), DomainError> {
        self.files.cleanup_user_backup(archive_path)
    }
}
