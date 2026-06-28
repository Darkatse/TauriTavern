use async_trait::async_trait;
use std::path::{Path, PathBuf};

use crate::domain::errors::DomainError;
use crate::domain::models::data_archive::{
    DataArchiveImportFailure, DataArchiveLocalMutationSummary,
};

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

#[async_trait]
pub(crate) trait DataRootInitializer: Send + Sync {
    async fn initialize_data_root(&self, data_root: &Path) -> Result<(), DomainError>;
}
