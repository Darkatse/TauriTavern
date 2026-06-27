use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(target_os = "ios")]
use crate::application::dto::data_archive_dto::{
    DATA_ARCHIVE_KIND_EXPORT, DATA_ARCHIVE_STATE_COMPLETED,
};
use crate::application::dto::data_archive_dto::{DataArchiveJobStatus, UserBackupArchiveResult};
use crate::domain::errors::DomainError;

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
    fn get_status(&self, job_id: &str) -> Result<DataArchiveJobStatus, DomainError>;
    fn cancel(&self, job_id: &str) -> Result<(), DomainError>;
    fn cleanup_export(&self, job_id: &str) -> Result<(), DomainError>;
    fn save_export(&self, job_id: &str) -> Result<PathBuf, DomainError>;
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
    backend: Arc<dyn DataArchiveJobBackend>,
}

impl DataArchiveService {
    pub(crate) fn new(backend: Arc<dyn DataArchiveJobBackend>) -> Self {
        Self { backend }
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
        self.backend.get_status(job_id)
    }

    #[cfg(target_os = "ios")]
    pub fn completed_export_archive_path(&self, job_id: &str) -> Result<PathBuf, DomainError> {
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

    pub fn cancel(&self, job_id: &str) -> Result<(), DomainError> {
        self.backend.cancel(job_id)
    }

    pub fn cleanup_export(&self, job_id: &str) -> Result<(), DomainError> {
        self.backend.cleanup_export(job_id)
    }

    pub async fn save_export(&self, job_id: String) -> Result<PathBuf, DomainError> {
        let backend = self.backend.clone();
        run_blocking("Save export task join error", move || {
            backend.save_export(&job_id)
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
