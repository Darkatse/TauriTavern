use super::FileSkillRepository;
use super::fs_ops::{activate_package_dir, cleanup_dir};
use super::materialize::PreparedImport;
use super::package::{ValidatedSkill, validate_skill_root};
use super::source_refs::merge_source_refs;
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillImportConflict, SkillImportConflictKind, SkillInstallAction, SkillInstallConflictStrategy,
    SkillInstallResult,
};

impl FileSkillRepository {
    pub(super) async fn preview_prepared(
        &self,
        prepared: &PreparedImport,
    ) -> Result<ValidatedSkill, DomainError> {
        let mut validated = validate_skill_root(&prepared.package_root, prepared.source.clone())?;
        let index = self.load_index().await?;
        let installed = index
            .skills
            .iter()
            .find(|skill| skill.name == validated.entry.name);
        validated.preview.conflict = match installed {
            None => SkillImportConflict {
                kind: SkillImportConflictKind::New,
                installed_hash: None,
            },
            Some(entry) if entry.installed_hash == validated.entry.installed_hash => {
                SkillImportConflict {
                    kind: SkillImportConflictKind::Same,
                    installed_hash: Some(entry.installed_hash.clone()),
                }
            }
            Some(entry) => SkillImportConflict {
                kind: SkillImportConflictKind::Different,
                installed_hash: Some(entry.installed_hash.clone()),
            },
        };
        Ok(validated)
    }

    pub(super) async fn install_validated(
        &self,
        prepared: PreparedImport,
        validated: ValidatedSkill,
        strategy: Option<SkillInstallConflictStrategy>,
    ) -> Result<SkillInstallResult, DomainError> {
        let mut index = self.load_index().await?;
        let existing_position = index
            .skills
            .iter()
            .position(|skill| skill.name == validated.entry.name);

        match validated.preview.conflict.kind {
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
                        self.save_index(&index).await?;
                        Some(index.skills[position].clone())
                    }
                    None => None,
                };
                cleanup_dir(&prepared.cleanup_root);
                return Ok(SkillInstallResult {
                    name: validated.entry.name,
                    action: SkillInstallAction::AlreadyInstalled,
                    skill,
                });
            }
            SkillImportConflictKind::Different => match strategy {
                Some(SkillInstallConflictStrategy::Skip) => {
                    cleanup_dir(&prepared.cleanup_root);
                    return Ok(SkillInstallResult {
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

        let target = self.installed_root().join(&validated.entry.name);
        let replaced = target.exists();
        activate_package_dir(&prepared.package_root, &target)?;
        cleanup_dir(&prepared.cleanup_root);

        match existing_position {
            Some(position) => index.skills[position] = validated.entry.clone(),
            None => index.skills.push(validated.entry.clone()),
        }
        index
            .skills
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.save_index(&index).await?;

        Ok(SkillInstallResult {
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
