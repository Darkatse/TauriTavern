use crate::dto::data_archive_dto::{
    DATA_ARCHIVE_ARTIFACT_AVAILABLE, DATA_ARCHIVE_ARTIFACT_DISPOSED, DATA_ARCHIVE_KIND_EXPORT,
    DATA_ARCHIVE_STATE_COMPLETED, UserBackupArchiveResult,
};
use tt_domain::errors::DomainError;

use super::{CompletedExportArchive, DataArchiveService, run_blocking};

impl DataArchiveService {
    /// Removes the artifact from the available set before opening a native dialog.
    pub fn claim_export_artifact(
        &self,
        job_id: &str,
    ) -> Result<CompletedExportArchive, DomainError> {
        let job = self.jobs.get(job_id)?;
        let status = job.snapshot()?;
        if status.kind != DATA_ARCHIVE_KIND_EXPORT || status.state != DATA_ARCHIVE_STATE_COMPLETED {
            return Err(DomainError::InvalidData(format!(
                "Export job is not completed: {job_id}"
            )));
        }
        let result = status.result.ok_or_else(|| {
            DomainError::InvalidData(format!("Export job has no result: {job_id}"))
        })?;
        match result.artifact_state.as_deref() {
            Some(DATA_ARCHIVE_ARTIFACT_AVAILABLE) => {}
            Some(DATA_ARCHIVE_ARTIFACT_DISPOSED) => {
                return Err(DomainError::InvalidData(format!(
                    "Export archive has already been handled: {job_id}"
                )));
            }
            state => {
                return Err(DomainError::InvalidData(format!(
                    "Invalid export artifact state for {job_id}: {state:?}"
                )));
            }
        }
        let file_name = result.file_name.ok_or_else(|| {
            DomainError::InvalidData(format!("Export job has no file name: {job_id}"))
        })?;
        let archive_path = job.claim_export_artifact_path()?.ok_or_else(|| {
            DomainError::InvalidData(format!("Export archive is already being handled: {job_id}"))
        })?;
        Ok(CompletedExportArchive {
            archive_path,
            file_name,
        })
    }

    /// Delivery consumes an artifact on success, cancellation, and failure alike.
    pub fn finish_export_delivery(&self, job_id: &str) -> Result<(), DomainError> {
        self.jobs.get(job_id)?.mark_export_artifact_disposed()
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
}
