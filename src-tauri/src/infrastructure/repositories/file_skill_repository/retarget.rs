use std::path::{Path, PathBuf};

use super::FileSkillRepository;
use super::fs_ops::{
    copy_skill_dir_to_empty_target, delete_installed_skill_dir, ensure_installed_skill_dir,
    rollback_prepared_skill_dirs,
};
use super::index::sort_index;
use super::paths::validate_skill_scope;
use super::source_refs::{merge_source_refs, sort_dedup_source_refs};
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillIndexEntry, SkillScope, SkillScopeRetargetRequest, SkillScopeRetargetResult,
};

#[derive(Debug, Clone)]
struct ScopeSourceLink {
    kind: &'static str,
    id: String,
    label: String,
}

enum RetargetFsAction {
    Copy {
        name: String,
        source_root: PathBuf,
        target_root: PathBuf,
    },
    DeleteSource {
        name: String,
        source_root: PathBuf,
    },
}

impl RetargetFsAction {
    fn source_root(&self) -> &Path {
        match self {
            Self::Copy { source_root, .. } | Self::DeleteSource { source_root, .. } => source_root,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Copy { name, .. } | Self::DeleteSource { name, .. } => name,
        }
    }
}

pub(super) async fn retarget_scope(
    repository: &FileSkillRepository,
    request: SkillScopeRetargetRequest,
) -> Result<SkillScopeRetargetResult, DomainError> {
    validate_skill_scope(&request.from_scope)?;
    validate_skill_scope(&request.to_scope)?;
    if request.from_scope == request.to_scope {
        return Err(DomainError::InvalidData(
            "from_scope and to_scope must differ".to_string(),
        ));
    }

    let (from_link, to_link) = scope_source_link_pair(&request.from_scope, &request.to_scope)?;
    let mut index = repository.load_index().await?;
    preflight_target_conflicts(&index.skills, &request.from_scope, &request.to_scope)?;

    let actions = build_fs_actions(
        repository,
        &index.skills,
        &request.from_scope,
        &request.to_scope,
    )?;
    let mut source_ref_entries_updated = 0usize;
    let mut target_and_unrelated_skills = Vec::new();
    let mut from_scope_skills = Vec::new();
    for mut skill in index.skills {
        if retarget_source_refs(&mut skill, &from_link, &to_link) {
            source_ref_entries_updated += 1;
        }
        if skill.scope == request.from_scope {
            from_scope_skills.push(skill);
        } else {
            target_and_unrelated_skills.push(skill);
        }
    }

    let mut next_skills =
        Vec::with_capacity(target_and_unrelated_skills.len() + from_scope_skills.len());
    for skill in target_and_unrelated_skills {
        merge_skill_entry(&mut next_skills, skill)?;
    }
    for mut skill in from_scope_skills {
        skill.scope = request.to_scope.clone();
        merge_skill_entry(&mut next_skills, skill)?;
    }

    if actions.is_empty() && source_ref_entries_updated == 0 {
        return Ok(SkillScopeRetargetResult {
            moved: 0,
            merged: 0,
            source_refs_updated: source_ref_entries_updated,
        });
    }

    // Keep the source scope intact until the index commit succeeds. A failed
    // save rolls back only the prepared target copies; source cleanup happens
    // after the index points at the new scope.
    let prepared_targets = prepare_target_copies(&actions)?;
    index.skills = next_skills;
    sort_index(&mut index);
    if let Err(error) = repository.save_index(&index).await {
        return Err(rollback_prepared_skill_dirs(&prepared_targets, error));
    }

    cleanup_sources_after_commit(&actions)?;

    Ok(SkillScopeRetargetResult {
        moved: actions
            .iter()
            .filter(|action| matches!(action, RetargetFsAction::Copy { .. }))
            .count(),
        merged: actions
            .iter()
            .filter(|action| matches!(action, RetargetFsAction::DeleteSource { .. }))
            .count(),
        source_refs_updated: source_ref_entries_updated,
    })
}

fn scope_source_link_pair(
    from_scope: &SkillScope,
    to_scope: &SkillScope,
) -> Result<(ScopeSourceLink, ScopeSourceLink), DomainError> {
    match (from_scope, to_scope) {
        (
            SkillScope::Preset {
                api_id: from_api_id,
                name: from_name,
            },
            SkillScope::Preset {
                api_id: to_api_id,
                name: to_name,
            },
        ) if from_api_id == to_api_id => Ok((
            ScopeSourceLink {
                kind: "preset",
                id: from_scope.label(),
                label: from_name.trim().to_string(),
            },
            ScopeSourceLink {
                kind: "preset",
                id: to_scope.label(),
                label: to_name.trim().to_string(),
            },
        )),
        (SkillScope::Preset { .. }, SkillScope::Preset { .. }) => Err(DomainError::InvalidData(
            "Cannot retarget preset Skill scope across api_id".to_string(),
        )),
        (
            SkillScope::Character {
                character_id: from_character_id,
            },
            SkillScope::Character {
                character_id: to_character_id,
            },
        ) => Ok((
            ScopeSourceLink {
                kind: "character",
                id: from_scope.label(),
                label: from_character_id.trim().to_string(),
            },
            ScopeSourceLink {
                kind: "character",
                id: to_scope.label(),
                label: to_character_id.trim().to_string(),
            },
        )),
        _ => Err(DomainError::InvalidData(
            "retarget_skill_scope supports preset and character scope renames only".to_string(),
        )),
    }
}

fn preflight_target_conflicts(
    skills: &[SkillIndexEntry],
    from_scope: &SkillScope,
    to_scope: &SkillScope,
) -> Result<(), DomainError> {
    for skill in skills.iter().filter(|skill| skill.scope == *from_scope) {
        if let Some(target) = skills
            .iter()
            .find(|candidate| candidate.scope == *to_scope && candidate.name == skill.name)
        {
            if target.installed_hash != skill.installed_hash {
                return Err(DomainError::InvalidData(format!(
                    "Skill '{}' already exists in target scope with different content",
                    skill.name
                )));
            }
        }
    }
    Ok(())
}

fn build_fs_actions(
    repository: &FileSkillRepository,
    skills: &[SkillIndexEntry],
    from_scope: &SkillScope,
    to_scope: &SkillScope,
) -> Result<Vec<RetargetFsAction>, DomainError> {
    let mut actions = Vec::new();
    for skill in skills.iter().filter(|skill| skill.scope == *from_scope) {
        let source_root = repository
            .installed_scope_root(from_scope)?
            .join(&skill.name);
        ensure_installed_skill_dir(&source_root, &skill.name)?;
        let target_exists = skills
            .iter()
            .any(|candidate| candidate.scope == *to_scope && candidate.name == skill.name);
        if target_exists {
            actions.push(RetargetFsAction::DeleteSource {
                name: skill.name.clone(),
                source_root,
            });
            continue;
        }

        let target_root = repository.installed_scope_root(to_scope)?.join(&skill.name);
        if target_root.exists() {
            return Err(DomainError::InvalidData(format!(
                "Skill target directory already exists without an index entry: {}",
                target_root.display()
            )));
        }
        actions.push(RetargetFsAction::Copy {
            name: skill.name.clone(),
            source_root,
            target_root,
        });
    }
    Ok(actions)
}

fn retarget_source_refs(
    skill: &mut SkillIndexEntry,
    from_link: &ScopeSourceLink,
    to_link: &ScopeSourceLink,
) -> bool {
    let mut changed = false;
    for reference in &mut skill.source_refs {
        if reference.kind == from_link.kind && reference.id == from_link.id {
            reference.id = to_link.id.clone();
            reference.label = to_link.label.clone();
            changed = true;
        }
    }
    if changed {
        let mut retained_target = false;
        skill.source_refs.retain(|reference| {
            if reference.kind == to_link.kind && reference.id == to_link.id {
                if retained_target {
                    return false;
                }
                retained_target = true;
            }
            true
        });
        if let Some(reference) = skill
            .source_refs
            .iter_mut()
            .find(|reference| reference.kind == to_link.kind && reference.id == to_link.id)
        {
            reference.label = to_link.label.clone();
        }
        sort_dedup_source_refs(&mut skill.source_refs);
    }
    changed
}

fn merge_skill_entry(
    skills: &mut Vec<SkillIndexEntry>,
    skill: SkillIndexEntry,
) -> Result<(), DomainError> {
    let Some(existing) = skills
        .iter_mut()
        .find(|existing| existing.scope == skill.scope && existing.name == skill.name)
    else {
        skills.push(skill);
        return Ok(());
    };

    if existing.installed_hash != skill.installed_hash {
        return Err(DomainError::InvalidData(format!(
            "Skill '{}' already exists in target scope with different content",
            skill.name
        )));
    }

    merge_source_refs(&mut existing.source_refs, skill.source_refs);
    sort_dedup_source_refs(&mut existing.source_refs);
    Ok(())
}

fn prepare_target_copies(actions: &[RetargetFsAction]) -> Result<Vec<PathBuf>, DomainError> {
    let mut prepared_targets = Vec::new();
    for action in actions {
        let RetargetFsAction::Copy {
            source_root,
            target_root,
            name,
        } = action
        else {
            continue;
        };

        if let Err(error) = copy_skill_dir_to_empty_target(source_root, target_root, name) {
            return Err(rollback_prepared_skill_dirs(&prepared_targets, error));
        }
        prepared_targets.push(target_root.clone());
    }
    Ok(prepared_targets)
}

fn cleanup_sources_after_commit(actions: &[RetargetFsAction]) -> Result<(), DomainError> {
    let mut errors = Vec::new();
    for action in actions {
        if let Err(error) = delete_installed_skill_dir(action.source_root(), action.name()) {
            errors.push(format!("{}: {}", action.source_root().display(), error));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(DomainError::InternalError(format!(
            "retarget_skill_scope committed but failed to clean up Skill directories: {}",
            errors.join("; ")
        )))
    }
}
