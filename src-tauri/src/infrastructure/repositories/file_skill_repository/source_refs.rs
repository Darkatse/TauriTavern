use serde_json::Value;

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

pub(super) fn sort_dedup_source_refs(source_refs: &mut Vec<SkillSourceRef>) {
    source_refs.sort_by(|left, right| left.kind.cmp(&right.kind).then(left.id.cmp(&right.id)));
    source_refs.dedup_by(|left, right| left.kind == right.kind && left.id == right.id);
}
