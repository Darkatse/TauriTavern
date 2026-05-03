use std::path::PathBuf;

use tokio::fs as tokio_fs;

use super::FileSkillRepository;
use crate::domain::errors::DomainError;

impl FileSkillRepository {
    pub(super) fn installed_root(&self) -> PathBuf {
        self.root.join("installed")
    }

    pub(super) fn staging_root(&self) -> PathBuf {
        self.root.join(".staging")
    }

    pub(super) fn index_dir(&self) -> PathBuf {
        self.root.join("index")
    }

    pub(super) fn index_path(&self) -> PathBuf {
        self.index_dir().join("skills.json")
    }

    pub(super) async fn ensure_layout(&self) -> Result<(), DomainError> {
        tokio_fs::create_dir_all(self.installed_root())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Skill installed directory: {error}"
                ))
            })?;
        tokio_fs::create_dir_all(self.staging_root())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Skill staging directory: {error}"
                ))
            })?;
        tokio_fs::create_dir_all(self.index_dir())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Skill index directory: {error}"
                ))
            })?;
        Ok(())
    }
}
