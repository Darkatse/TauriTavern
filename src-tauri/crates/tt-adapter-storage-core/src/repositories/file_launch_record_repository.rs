use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;
use tt_domain::models::safe_mode::LaunchRecord;

use crate::file_system::persist_json_file_blocking;

pub struct FileLaunchRecordRepository {
    path: PathBuf,
}

impl FileLaunchRecordRepository {
    pub fn new(app_root: &Path) -> Self {
        Self {
            path: app_root.join("safe-mode.json"),
        }
    }

    pub fn load(&self) -> Result<LaunchRecord, DomainError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                DomainError::InvalidData(format!("Invalid {}: {error}", self.path.display()))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(LaunchRecord::default())
            }
            Err(error) => Err(DomainError::InternalError(format!(
                "Failed to read {}: {error}",
                self.path.display()
            ))),
        }
    }

    pub fn save(&self, record: &LaunchRecord) -> Result<(), DomainError> {
        persist_json_file_blocking(&self.path, record)
    }
}
