use std::path::PathBuf;

use tokio::fs as tokio_fs;

use super::FileSkillRepository;
use super::paths::skill_scope_storage_dir;
use crate::domain::errors::DomainError;
use crate::domain::models::skill::SkillScope;

impl FileSkillRepository {
    pub(super) fn installed_root(&self) -> PathBuf {
        self.root.join("installed")
    }

    pub(super) fn installed_scope_root(&self, scope: &SkillScope) -> Result<PathBuf, DomainError> {
        Ok(self.installed_root().join(skill_scope_storage_dir(scope)?))
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
