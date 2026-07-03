use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs as tokio_fs;

use super::fs_ops::{
    SkillDirCleanup, cleanup_committed_skill_dirs, copy_skill_dir_to_empty_target,
    ensure_installed_skill_dir, rollback_prepared_skill_dirs,
};
use super::package::validate_skill_root;
use super::paths::{validate_skill_name, validate_skill_scope};
use super::{FileSkillRepository, INDEX_VERSION};
use tt_adapter_storage_core::file_system::{replace_file_with_fallback, unique_temp_path};
use tt_domain::errors::DomainError;
use tt_domain::models::skill::{SkillIndexEntry, SkillScope};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SkillIndexFile {
    pub(super) version: u32,
    pub(super) skills: Vec<SkillIndexEntry>,
}

pub(super) enum SkillDirectoryState {
    Present,
    Missing,
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

    pub(super) async fn load_index_view_filtered_missing_dirs(
        &self,
    ) -> Result<SkillIndexFile, DomainError> {
        let mut index = self.load_index().await?;
        self.filter_missing_skill_dirs(&mut index, None)?;
        Ok(index)
    }

    pub(super) async fn load_index_import_view(
        &self,
        scope: &SkillScope,
        name: &str,
    ) -> Result<SkillIndexFile, DomainError> {
        let mut index = self.load_index().await?;
        self.reconcile_import_target(&mut index, scope, name)?;
        Ok(index)
    }

    pub(super) async fn repair_index_for_import_target(
        &self,
        scope: &SkillScope,
        name: &str,
    ) -> Result<SkillIndexFile, DomainError> {
        let mut index = self.load_index().await?;
        if self.reconcile_import_target(&mut index, scope, name)? {
            self.save_index(&index).await?;
        }
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

    pub(super) fn skill_directory_state(
        &self,
        skill: &SkillIndexEntry,
    ) -> Result<SkillDirectoryState, DomainError> {
        let root = self.installed_scope_root(&skill.scope)?.join(&skill.name);
        let metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SkillDirectoryState::Missing);
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read Skill directory metadata '{}': {}",
                    root.display(),
                    error
                )));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Skill directory cannot be a symlink: {}/{}",
                skill.scope.label(),
                skill.name
            )));
        }
        if !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Skill installed path is not a directory: {}/{}",
                skill.scope.label(),
                skill.name
            )));
        }
        Ok(SkillDirectoryState::Present)
    }

    fn filter_missing_skill_dirs(
        &self,
        index: &mut SkillIndexFile,
        target: Option<(&SkillScope, &str)>,
    ) -> Result<bool, DomainError> {
        let mut changed = false;
        let mut retained = Vec::with_capacity(index.skills.len());

        for skill in std::mem::take(&mut index.skills) {
            let should_check = match target {
                Some((scope, name)) => skill.scope == *scope && skill.name == name,
                None => true,
            };
            if !should_check {
                retained.push(skill);
                continue;
            }

            match self.skill_directory_state(&skill)? {
                SkillDirectoryState::Present => retained.push(skill),
                SkillDirectoryState::Missing => {
                    changed = true;
                    tracing::warn!(
                        "Pruning stale Skill index entry for missing directory: {}/{}",
                        skill.scope.label(),
                        skill.name
                    );
                }
            }
        }

        index.skills = retained;
        if changed {
            sort_index(index);
        }
        Ok(changed)
    }

    fn reconcile_import_target(
        &self,
        index: &mut SkillIndexFile,
        scope: &SkillScope,
        name: &str,
    ) -> Result<bool, DomainError> {
        let mut changed = self.filter_missing_skill_dirs(index, Some((scope, name)))?;
        let has_entry = index
            .skills
            .iter()
            .any(|skill| skill.scope == *scope && skill.name == name);
        if has_entry {
            return Ok(changed);
        }

        let Some(entry) = self.validated_orphan_import_target(scope, name)? else {
            return Ok(changed);
        };
        index.skills.push(entry);
        sort_index(index);
        changed = true;
        Ok(changed)
    }

    fn validated_orphan_import_target(
        &self,
        scope: &SkillScope,
        name: &str,
    ) -> Result<Option<SkillIndexEntry>, DomainError> {
        let name = validate_skill_name(name)?;
        validate_skill_scope(scope)?;
        let root = self.installed_scope_root(scope)?.join(&name);
        let metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read Skill directory metadata '{}': {}",
                    root.display(),
                    error
                )));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Skill directory cannot be a symlink: {}/{}",
                scope.label(),
                name
            )));
        }
        if !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Skill installed path is not a directory: {}/{}",
                scope.label(),
                name
            )));
        }

        let mut entry = validate_skill_root(&root, Value::Null)?.entry;
        if entry.name != name {
            return Err(DomainError::InvalidData(format!(
                "Skill directory name '{}' does not match SKILL.md name '{}'",
                name, entry.name
            )));
        }
        entry.scope = scope.clone();
        entry.source_refs.clear();
        tracing::warn!(
            "Restoring missing Skill index entry from installed directory: {}/{}",
            entry.scope.label(),
            entry.name
        );
        Ok(Some(entry))
    }

    async fn migrate_v1_index(
        &self,
        mut index: SkillIndexFile,
    ) -> Result<SkillIndexFile, DomainError> {
        let mut prepared_targets: Vec<PathBuf> = Vec::new();
        let mut legacy_dirs = Vec::new();

        for skill in &mut index.skills {
            if let Err(error) = validate_skill_name(&skill.name) {
                return Err(rollback_prepared_skill_dirs(&prepared_targets, error));
            }
            skill.scope = SkillScope::Global;
            let old_root = self.installed_root().join(&skill.name);
            let new_scope_root = self.installed_scope_root(&SkillScope::Global)?;
            let new_root = new_scope_root.join(&skill.name);
            let old_exists = old_root.exists();
            let new_exists = new_root.exists();

            match (old_exists, new_exists) {
                (true, false) => {
                    if let Err(error) = ensure_installed_skill_dir(&old_root, &skill.name) {
                        return Err(rollback_prepared_skill_dirs(&prepared_targets, error));
                    }
                    if let Err(error) =
                        copy_skill_dir_to_empty_target(&old_root, &new_root, &skill.name)
                    {
                        let migration_error = DomainError::InternalError(format!(
                            "Failed to migrate Skill '{}' into global scope: {}",
                            skill.name, error
                        ));
                        return Err(rollback_prepared_skill_dirs(
                            &prepared_targets,
                            migration_error,
                        ));
                    }
                    prepared_targets.push(new_root);
                    legacy_dirs.push(SkillDirCleanup {
                        name: skill.name.clone(),
                        path: old_root,
                    });
                }
                (false, true) => {
                    if let Err(error) = ensure_installed_skill_dir(&new_root, &skill.name) {
                        return Err(rollback_prepared_skill_dirs(&prepared_targets, error));
                    }
                    tracing::warn!(
                        "Skill index v1 migration found '{}' already under global scope while the legacy directory is missing; treating it as a completed prior move",
                        skill.name
                    );
                }
                (true, true) => {
                    return Err(rollback_prepared_skill_dirs(
                        &prepared_targets,
                        DomainError::InvalidData(format!(
                            "Skill index v1 migration target already exists for '{}'",
                            skill.name
                        )),
                    ));
                }
                (false, false) => {
                    return Err(rollback_prepared_skill_dirs(
                        &prepared_targets,
                        DomainError::NotFound(format!(
                            "Skill directory not found during v1 migration: {}",
                            skill.name
                        )),
                    ));
                }
            }
        }

        index.version = INDEX_VERSION;
        sort_index(&mut index);
        if let Err(error) = validate_index(&index) {
            return Err(rollback_prepared_skill_dirs(&prepared_targets, error));
        }
        if let Err(error) = self.save_index(&index).await {
            return Err(rollback_prepared_skill_dirs(&prepared_targets, error));
        }
        cleanup_committed_skill_dirs("migrate_v1_index", &legacy_dirs)?;
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
