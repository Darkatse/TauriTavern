mod export;
mod import;
mod shared;

use std::path::PathBuf;

use tt_domain::models::data_archive::DataArchiveLocalMutationSummary;

pub(crate) use export::{run_export_data_archive, run_export_user_backup_archive};
pub(crate) use import::run_import_data_archive;

#[derive(Debug, Clone)]
pub(crate) struct DataArchiveImportResult {
    pub(crate) source_users: Vec<String>,
    pub(crate) target_user: String,
    pub(crate) local_applied: DataArchiveLocalMutationSummary,
}

#[derive(Debug, Clone)]
pub(crate) struct DataArchiveExportResult {
    pub(crate) archive_path: PathBuf,
}
