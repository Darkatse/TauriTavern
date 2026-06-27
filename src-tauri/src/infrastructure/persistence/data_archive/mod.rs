mod export;
mod import;
mod shared;

use std::path::PathBuf;

pub use export::{
    default_export_file_name, run_export_data_archive, run_export_user_backup_archive,
};
pub use import::run_import_data_archive;

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
