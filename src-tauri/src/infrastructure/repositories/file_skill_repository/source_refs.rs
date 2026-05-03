use serde_json::Value;

use super::FileSkillRepository;
use super::fs_ops::delete_installed_skill_dir;
use super::paths::normalize_source_string;
use crate::domain::errors::DomainError;
use crate::domain::models::skill::SkillSourceRef;

pub(super) fn skill_source_ref_from_import_source(
    source: &Value,
    installed_hash: &str,
) -> Result<Option<SkillSourceRef>, DomainError> {
    let Some(kind) = source
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let id = source
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(id) = id else {
        if matches!(kind, "preset" | "character") {
            return Err(DomainError::InvalidData(format!(
                "Skill import source.id is required for source kind '{kind}'"
            )));
        }
        return Ok(None);
    };

    let kind = normalize_source_string(kind, "source kind")?;
    let id = normalize_source_string(id, "source id")?;
    let label = source
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| id.clone());

    Ok(Some(SkillSourceRef {
        kind,
        id,
        label,
        installed_hash: installed_hash.to_string(),
    }))
}

pub(super) fn merge_source_refs(
    target: &mut Vec<SkillSourceRef>,
    source_refs: Vec<SkillSourceRef>,
) {
    for source_ref in source_refs {
        target.retain(|existing| existing.kind != source_ref.kind || existing.id != source_ref.id);
        target.push(source_ref);
    }
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
        if skill.source_refs.is_empty() {
            delete_installed_skill_dir(
                &repository.installed_root().join(&skill.name),
                &skill.name,
            )?;
            deleted.push(skill.name);
        } else {
            next_skills.push(skill);
        }
    }

    if changed {
        next_skills.sort_by(|left, right| left.name.cmp(&right.name));
        index.skills = next_skills;
        repository.save_index(&index).await?;
    }

    Ok(deleted)
}
