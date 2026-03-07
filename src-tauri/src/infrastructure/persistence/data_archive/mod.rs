mod export;
mod import;
mod shared;

use std::path::PathBuf;

use crate::domain::errors::DomainError;

pub use export::{default_export_file_name, run_export_data_archive};
pub use import::run_import_data_archive;

pub const CANCELLED_ERROR_MARKER: &str = "__data_archive_job_cancelled__";

#[derive(Debug, Clone)]
pub struct DataArchiveImportResult {
    pub source_users: Vec<String>,
    pub target_user: String,
}

#[derive(Debug, Clone)]
pub struct DataArchiveExportResult {
    pub file_name: String,
    pub archive_path: PathBuf,
}

pub fn is_cancelled_error(error: &DomainError) -> bool {
    match error {
        DomainError::InternalError(message) => message == CANCELLED_ERROR_MARKER,
        _ => false,
    }
}

