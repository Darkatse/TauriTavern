use serde::{Deserialize, Serialize};
use tokio::fs as tokio_fs;

use super::paths::validate_skill_name;
use super::{FileSkillRepository, INDEX_VERSION};
use crate::domain::errors::DomainError;
use crate::domain::models::skill::SkillIndexEntry;
use crate::infrastructure::persistence::file_system::{
    replace_file_with_fallback, unique_temp_path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SkillIndexFile {
    pub(super) version: u32,
    pub(super) skills: Vec<SkillIndexEntry>,
}

impl FileSkillRepository {
    pub(super) async fn load_index(&self) -> Result<SkillIndexFile, DomainError> {
        self.ensure_layout().await?;
        let path = self.index_path();
        let text = match tokio_fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if self.has_installed_skill_directories().await? {
                    return Err(DomainError::InvalidData(
                        "Skill index is missing while installed skills exist".to_string(),
                    ));
                }
                let index = SkillIndexFile {
                    version: INDEX_VERSION,
                    skills: Vec::new(),
                };
                self.save_index(&index).await?;
                return Ok(index);
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read Skill index '{}': {}",
                    path.display(),
                    error
                )));
            }
        };

        let index: SkillIndexFile = serde_json::from_str(&text).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid Skill index '{}': {}",
                path.display(),
                error
            ))
        })?;
        if index.version != INDEX_VERSION {
            return Err(DomainError::InvalidData(format!(
                "Unsupported Skill index version {}",
                index.version
            )));
        }
        for skill in &index.skills {
            validate_skill_name(&skill.name)?;
        }
        Ok(index)
    }

    pub(super) async fn save_index(&self, index: &SkillIndexFile) -> Result<(), DomainError> {
        self.ensure_layout().await?;
        let path = self.index_path();
        let text = serde_json::to_string_pretty(index).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize Skill index: {error}"))
        })?;
        let temp = unique_temp_path(&path, "skills.json");
        tokio_fs::write(&temp, text).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write temporary Skill index '{}': {}",
                temp.display(),
                error
            ))
        })?;
        replace_file_with_fallback(&temp, &path).await?;
        Ok(())
    }

    async fn has_installed_skill_directories(&self) -> Result<bool, DomainError> {
        let mut entries = match tokio_fs::read_dir(self.installed_root()).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read Skill installed directory: {error}"
                )));
            }
        };
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read Skill installed directory entry: {error}"
            ))
        })? {
            let metadata = entry.metadata().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read Skill installed entry metadata: {error}"
                ))
            })?;
            if metadata.is_dir() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
