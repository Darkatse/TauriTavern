mod export;
mod import;
mod shared;

use std::path::PathBuf;

use crate::domain::errors::DomainError;

pub use export::{default_export_file_name, run_export_data_archive};
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

pub fn is_cancelled_error(error: &DomainError) -> bool {
    matches!(error, DomainError::Cancelled(_))
}

#[cfg(test)]
mod tests {
    use crate::domain::errors::GENERATION_CANCELLED_BY_USER_MESSAGE;

    use super::*;

    #[test]
    fn is_cancelled_error_accepts_cancelled_variant() {
        assert!(is_cancelled_error(&DomainError::cancelled("Job cancelled")));
    }

    #[test]
    fn is_cancelled_error_rejects_other_variants() {
        assert!(!is_cancelled_error(&DomainError::InternalError(
            GENERATION_CANCELLED_BY_USER_MESSAGE.to_string()
        )));
    }
}
