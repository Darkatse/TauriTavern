use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tokio::fs as tokio_fs;

use super::paths::{validate_skill_name, validate_skill_scope};
use super::{FileSkillRepository, INDEX_VERSION};
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{SkillIndexEntry, SkillScope};
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
        if index.version == 1 {
            return self.migrate_v1_index(index).await;
        }
        if index.version != INDEX_VERSION {
            return Err(DomainError::InvalidData(format!(
                "Unsupported Skill index version {}",
                index.version
            )));
        }
        validate_index(&index)?;
        Ok(index)
    }

    pub(super) async fn save_index(&self, index: &SkillIndexFile) -> Result<(), DomainError> {
        self.ensure_layout().await?;
        validate_index(index)?;
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

    async fn migrate_v1_index(
        &self,
        mut index: SkillIndexFile,
    ) -> Result<SkillIndexFile, DomainError> {
        for skill in &mut index.skills {
            validate_skill_name(&skill.name)?;
            skill.scope = SkillScope::Global;
            let old_root = self.installed_root().join(&skill.name);
            let new_scope_root = self.installed_scope_root(&SkillScope::Global)?;
            let new_root = new_scope_root.join(&skill.name);
            let old_exists = old_root.exists();
            let new_exists = new_root.exists();

            match (old_exists, new_exists) {
                (true, false) => {
                    tokio_fs::create_dir_all(&new_scope_root)
                        .await
                        .map_err(|error| {
                            DomainError::InternalError(format!(
                                "Failed to create global Skill scope directory '{}': {}",
                                new_scope_root.display(),
                                error
                            ))
                        })?;
                    tokio_fs::rename(&old_root, &new_root)
                        .await
                        .map_err(|error| {
                            DomainError::InternalError(format!(
                                "Failed to migrate Skill '{}' into global scope: {}",
                                skill.name, error
                            ))
                        })?;
                }
                (false, true) => {
                    return Err(DomainError::InvalidData(format!(
                        "Skill index v1 is partially migrated for '{}'",
                        skill.name
                    )));
                }
                (true, true) => {
                    return Err(DomainError::InvalidData(format!(
                        "Skill index v1 migration target already exists for '{}'",
                        skill.name
                    )));
                }
                (false, false) => {
                    return Err(DomainError::NotFound(format!(
                        "Skill directory not found during v1 migration: {}",
                        skill.name
                    )));
                }
            }
        }

        index.version = INDEX_VERSION;
        sort_index(&mut index);
        validate_index(&index)?;
        self.save_index(&index).await?;
        Ok(index)
    }
}

pub(super) fn sort_index(index: &mut SkillIndexFile) {
    index.skills.sort_by(|left, right| {
        left.scope
            .stable_key()
            .cmp(&right.scope.stable_key())
            .then(left.name.cmp(&right.name))
    });
}

pub(super) fn validate_index(index: &SkillIndexFile) -> Result<(), DomainError> {
    let mut keys = BTreeSet::new();
    for skill in &index.skills {
        validate_skill_name(&skill.name)?;
        validate_skill_scope(&skill.scope)?;
        let key = (skill.scope.stable_key(), skill.name.clone());
        if !keys.insert(key) {
            return Err(DomainError::InvalidData(format!(
                "Duplicate Skill index entry: {}/{}",
                skill.scope.label(),
                skill.name
            )));
        }
    }
    Ok(())
}
