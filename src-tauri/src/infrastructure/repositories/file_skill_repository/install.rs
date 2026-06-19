use super::FileSkillRepository;
use super::fs_ops::{
    cleanup_dir, copy_skill_dir_to_empty_target, prepare_skill_dir_replacement,
    rollback_prepared_skill_dir, rollback_prepared_skill_dir_replacement,
};
use super::index::{SkillIndexFile, sort_index};
use super::materialize::PreparedImport;
use super::package::{ValidatedSkill, validate_skill_root};
use super::source_refs::merge_source_refs;
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillImportConflict, SkillImportConflictKind, SkillInstallAction, SkillInstallConflictStrategy,
    SkillInstallResult, SkillScope,
};

impl FileSkillRepository {
    pub(super) async fn preview_prepared(
        &self,
        prepared: &PreparedImport,
        target_scope: SkillScope,
    ) -> Result<ValidatedSkill, DomainError> {
        let mut validated = validate_skill_root(&prepared.package_root, prepared.source.clone())?;
        validated.entry.scope = target_scope;
        validated.preview.skill = validated.entry.clone();
        let index = self
            .load_index_import_view(&validated.entry.scope, &validated.entry.name)
            .await?;
        validated.preview.conflict = import_conflict(&validated.entry, &index);
        Ok(validated)
    }

    pub(super) async fn install_validated(
        &self,
        prepared: PreparedImport,
        validated: ValidatedSkill,
        strategy: Option<SkillInstallConflictStrategy>,
    ) -> Result<SkillInstallResult, DomainError> {
        let mut index = match self
            .repair_index_for_import_target(&validated.entry.scope, &validated.entry.name)
            .await
        {
            Ok(index) => index,
            Err(error) => {
                cleanup_dir(&prepared.cleanup_root);
                return Err(error);
            }
        };
        let existing_position = index.skills.iter().position(|skill| {
            skill.scope == validated.entry.scope && skill.name == validated.entry.name
        });
        let conflict = import_conflict(&validated.entry, &index);

        match conflict.kind {
            SkillImportConflictKind::Same => {
                let skill = match existing_position {
                    Some(position) => {
                        merge_source_refs(
                            &mut index.skills[position].source_refs,
                            validated.entry.source_refs,
                        );
                        index.skills[position].source_refs.sort_by(|left, right| {
                            left.kind.cmp(&right.kind).then(left.id.cmp(&right.id))
                        });
                        if let Err(error) = self.save_index(&index).await {
                            cleanup_dir(&prepared.cleanup_root);
                            return Err(error);
                        }
                        Some(index.skills[position].clone())
                    }
                    None => None,
                };
                cleanup_dir(&prepared.cleanup_root);
                return Ok(SkillInstallResult {
                    scope: validated.entry.scope,
                    name: validated.entry.name,
                    action: SkillInstallAction::AlreadyInstalled,
                    skill,
                });
            }
            SkillImportConflictKind::Different => match strategy {
                Some(SkillInstallConflictStrategy::Skip) => {
                    cleanup_dir(&prepared.cleanup_root);
                    return Ok(SkillInstallResult {
                        scope: validated.entry.scope,
                        name: validated.entry.name,
                        action: SkillInstallAction::Skipped,
                        skill: existing_position.map(|position| index.skills[position].clone()),
                    });
                }
                Some(SkillInstallConflictStrategy::Replace) => {}
                None => {
                    cleanup_dir(&prepared.cleanup_root);
                    return Err(DomainError::InvalidData(format!(
                        "Skill '{}' already exists with different content; conflict_strategy is required",
                        validated.entry.name
                    )));
                }
            },
            SkillImportConflictKind::New => {}
        }

        let target = match self.installed_scope_root(&validated.entry.scope) {
            Ok(scope_root) => scope_root.join(&validated.entry.name),
            Err(error) => {
                cleanup_dir(&prepared.cleanup_root);
                return Err(error);
            }
        };
        if let Some(parent) = target.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                cleanup_dir(&prepared.cleanup_root);
                return Err(DomainError::InternalError(format!(
                    "Failed to create Skill scope directory '{}': {}",
                    parent.display(),
                    error
                )));
            }
        }
        match existing_position {
            Some(position) => index.skills[position] = validated.entry.clone(),
            None => index.skills.push(validated.entry.clone()),
        }
        sort_index(&mut index);

        let replaced = existing_position.is_some();
        if replaced {
            let replacement = match prepare_skill_dir_replacement(
                &prepared.package_root,
                &target,
                &validated.entry.name,
            ) {
                Ok(replacement) => replacement,
                Err(error) => {
                    cleanup_dir(&prepared.cleanup_root);
                    return Err(error);
                }
            };
            if let Err(error) = self.save_index(&index).await {
                let error = rollback_prepared_skill_dir_replacement(&replacement, error);
                cleanup_dir(&prepared.cleanup_root);
                return Err(error);
            }
            if let Err(error) = replacement.discard_backup() {
                cleanup_dir(&prepared.cleanup_root);
                return Err(committed_cleanup_error("install_skill_import", error));
            }
        } else {
            if let Err(error) = copy_skill_dir_to_empty_target(
                &prepared.package_root,
                &target,
                &validated.entry.name,
            ) {
                cleanup_dir(&prepared.cleanup_root);
                return Err(error);
            }
            if let Err(error) = self.save_index(&index).await {
                let error = rollback_prepared_skill_dir(&target, error);
                cleanup_dir(&prepared.cleanup_root);
                return Err(error);
            }
        }
        cleanup_dir(&prepared.cleanup_root);

        Ok(SkillInstallResult {
            scope: validated.entry.scope.clone(),
            name: validated.entry.name.clone(),
            action: if replaced {
                SkillInstallAction::Replaced
            } else {
                SkillInstallAction::Installed
            },
            skill: Some(validated.entry),
        })
    }
}

fn import_conflict(
    entry: &crate::domain::models::skill::SkillIndexEntry,
    index: &SkillIndexFile,
) -> SkillImportConflict {
    let installed = index
        .skills
        .iter()
        .find(|skill| skill.scope == entry.scope && skill.name == entry.name);
    match installed {
        None => SkillImportConflict {
            kind: SkillImportConflictKind::New,
            installed_hash: None,
        },
        Some(installed) if installed.installed_hash == entry.installed_hash => {
            SkillImportConflict {
                kind: SkillImportConflictKind::Same,
                installed_hash: Some(installed.installed_hash.clone()),
            }
        }
        Some(installed) => SkillImportConflict {
            kind: SkillImportConflictKind::Different,
            installed_hash: Some(installed.installed_hash.clone()),
        },
    }
}

fn committed_cleanup_error(operation: &str, error: DomainError) -> DomainError {
    DomainError::InternalError(format!(
        "{operation} committed but failed to clean up Skill directories: {error}"
    ))
}
