use super::FileSkillRepository;
use super::fs_ops::{
    PreparedSkillDirReplacement, SkillDirCleanup, cleanup_committed_skill_dirs,
    copy_skill_dir_to_empty_target, delete_installed_skill_dir, ensure_installed_skill_dir,
    prepare_skill_dir_replacement, rollback_prepared_skill_dir,
    rollback_prepared_skill_dir_replacement,
};
use super::index::SkillDirectoryState;
use super::index::sort_index;
use super::paths::{normalize_source_string, validate_skill_name, validate_skill_scope};
use super::source_refs::sort_dedup_source_refs;
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{
    SkillInstallAction, SkillInstallConflictStrategy, SkillInstallResult, SkillMoveRequest,
    SkillScope,
};

pub(super) async fn delete_skill(
    repository: &FileSkillRepository,
    scope: SkillScope,
    name: &str,
) -> Result<(), DomainError> {
    let name = validate_skill_name(name)?;
    validate_skill_scope(&scope)?;
    let mut index = repository.load_index().await?;
    let Some(position) = index
        .skills
        .iter()
        .position(|skill| skill.scope == scope && skill.name == name)
    else {
        return Err(DomainError::NotFound(format!(
            "Skill not found: {}/{}",
            scope.label(),
            name
        )));
    };

    let skill = index.skills[position].clone();
    let skill_root = repository.installed_scope_root(&scope)?.join(&name);
    let directory_present = matches!(
        repository.skill_directory_state(&skill)?,
        SkillDirectoryState::Present
    );
    index.skills.remove(position);
    repository.save_index(&index).await?;
    if directory_present {
        cleanup_committed_skill_dirs(
            "delete_skill",
            &[SkillDirCleanup {
                name,
                path: skill_root,
            }],
        )?;
    }
    Ok(())
}

pub(super) async fn move_skill(
    repository: &FileSkillRepository,
    request: SkillMoveRequest,
) -> Result<SkillInstallResult, DomainError> {
    let name = validate_skill_name(&request.name)?;
    validate_skill_scope(&request.from_scope)?;
    validate_skill_scope(&request.to_scope)?;
    if request.from_scope == request.to_scope {
        return Err(DomainError::InvalidData(
            "from_scope and to_scope must differ".to_string(),
        ));
    }

    let mut index = repository.load_index().await?;
    let Some(from_position) = index
        .skills
        .iter()
        .position(|skill| skill.scope == request.from_scope && skill.name == name)
    else {
        return Err(DomainError::NotFound(format!(
            "Skill not found: {}/{}",
            request.from_scope.label(),
            name
        )));
    };
    let to_position = index
        .skills
        .iter()
        .position(|skill| skill.scope == request.to_scope && skill.name == name);

    let source_root = repository
        .installed_scope_root(&request.from_scope)?
        .join(&name);
    let target_root = repository
        .installed_scope_root(&request.to_scope)?
        .join(&name);
    ensure_installed_skill_dir(&source_root, &name)?;

    let mut moved_entry = index.skills[from_position].clone();
    moved_entry.scope = request.to_scope.clone();

    if let Some(position) = to_position {
        let target_entry = index.skills[position].clone();
        ensure_installed_skill_dir(&target_root, &name)?;
        if target_entry.installed_hash == moved_entry.installed_hash {
            let mut target_entry = target_entry;
            target_entry.source_refs.extend(moved_entry.source_refs);
            sort_dedup_source_refs(&mut target_entry.source_refs);
            index.skills[position] = target_entry.clone();
            index.skills.remove(from_position);
            sort_index(&mut index);
            repository.save_index(&index).await?;
            cleanup_committed_skill_dirs(
                "move_skill",
                &[SkillDirCleanup {
                    name: name.clone(),
                    path: source_root,
                }],
            )?;
            return Ok(SkillInstallResult {
                scope: request.to_scope,
                name,
                action: SkillInstallAction::AlreadyInstalled,
                skill: Some(target_entry),
            });
        }

        match request.conflict_strategy {
            Some(SkillInstallConflictStrategy::Skip) => {
                return Ok(SkillInstallResult {
                    scope: request.to_scope,
                    name,
                    action: SkillInstallAction::Skipped,
                    skill: Some(target_entry),
                });
            }
            Some(SkillInstallConflictStrategy::Replace) => {}
            None => {
                return Err(DomainError::InvalidData(format!(
                    "Skill '{}' already exists in target scope with different content; conflict_strategy is required",
                    name
                )));
            }
        }

        let replacement = prepare_skill_dir_replacement(&source_root, &target_root, &name)?;
        index.skills[position] = moved_entry.clone();
        index.skills.remove(from_position);
        sort_index(&mut index);
        if let Err(error) = repository.save_index(&index).await {
            return Err(rollback_prepared_skill_dir_replacement(&replacement, error));
        }
        cleanup_move_replace_after_commit(
            SkillDirCleanup {
                name: name.clone(),
                path: source_root,
            },
            &replacement,
        )?;
        return Ok(SkillInstallResult {
            scope: request.to_scope,
            name,
            action: SkillInstallAction::Replaced,
            skill: Some(moved_entry),
        });
    }

    copy_skill_dir_to_empty_target(&source_root, &target_root, &name)?;
    index.skills[from_position] = moved_entry.clone();
    sort_index(&mut index);
    if let Err(error) = repository.save_index(&index).await {
        return Err(rollback_prepared_skill_dir(&target_root, error));
    }
    cleanup_committed_skill_dirs(
        "move_skill",
        &[SkillDirCleanup {
            name: name.clone(),
            path: source_root,
        }],
    )?;

    Ok(SkillInstallResult {
        scope: request.to_scope,
        name,
        action: SkillInstallAction::Installed,
        skill: Some(moved_entry),
    })
}

pub(super) async fn delete_skills_for_source(
    repository: &FileSkillRepository,
    source_kind: &str,
    source_id: &str,
) -> Result<Vec<String>, DomainError> {
    let source_kind = normalize_source_string(source_kind, "source kind")?;
    let source_id = normalize_source_string(source_id, "source id")?;
    let mut index = repository.load_index().await?;
    let mut next_skills = Vec::with_capacity(index.skills.len());
    let mut cleanup_dirs = Vec::new();
    let mut deleted = Vec::new();
    let mut changed = false;

    for mut skill in index.skills {
        let original_len = skill.source_refs.len();
        skill
            .source_refs
            .retain(|source| source.kind != source_kind || source.id != source_id);

        if skill.source_refs.len() == original_len {
            next_skills.push(skill);
            continue;
        }

        changed = true;
        let directory_present = matches!(
            repository.skill_directory_state(&skill)?,
            SkillDirectoryState::Present
        );
        if !directory_present {
            tracing::warn!(
                "Pruning stale Skill index entry while deleting source '{}:{}': {}/{}",
                source_kind,
                source_id,
                skill.scope.label(),
                skill.name
            );
            deleted.push(format!("{}/{}", skill.scope.label(), skill.name));
            continue;
        }

        if !skill.source_refs.is_empty() {
            next_skills.push(skill);
            continue;
        }

        let skill_root = repository
            .installed_scope_root(&skill.scope)?
            .join(&skill.name);
        deleted.push(format!("{}/{}", skill.scope.label(), skill.name));
        cleanup_dirs.push(SkillDirCleanup {
            name: skill.name,
            path: skill_root,
        });
    }

    if changed {
        index.skills = next_skills;
        sort_index(&mut index);
        repository.save_index(&index).await?;
        cleanup_committed_skill_dirs("delete_skills_for_source", &cleanup_dirs)?;
    }

    Ok(deleted)
}

fn cleanup_move_replace_after_commit(
    source_dir: SkillDirCleanup,
    replacement: &PreparedSkillDirReplacement,
) -> Result<(), DomainError> {
    let mut errors = Vec::new();
    if let Err(error) = replacement.discard_backup() {
        errors.push(error.to_string());
    }
    if let Err(error) = delete_installed_skill_dir(&source_dir.path, &source_dir.name) {
        errors.push(format!("{}: {}", source_dir.path.display(), error));
    }
    committed_cleanup_result("move_skill", errors)
}

fn committed_cleanup_result(operation: &str, errors: Vec<String>) -> Result<(), DomainError> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(DomainError::InternalError(format!(
            "{operation} committed but failed to clean up Skill directories: {}",
            errors.join("; ")
        )))
    }
}
