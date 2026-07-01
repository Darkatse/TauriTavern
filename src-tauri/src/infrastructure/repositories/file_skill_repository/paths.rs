use sha2::{Digest, Sha256};

use tt_domain::errors::DomainError;
use tt_domain::models::skill::{SkillScope, SkillScopeFilter};

pub(super) fn validate_skill_name(raw: &str) -> Result<String, DomainError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(
            "Skill name cannot be empty".to_string(),
        ));
    }
    if value.len() > 128 {
        return Err(DomainError::InvalidData(
            "Skill name must be <= 128 characters".to_string(),
        ));
    }
    if matches!(value, "." | "..") {
        return Err(DomainError::InvalidData(
            "Skill name cannot be '.' or '..'".to_string(),
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(DomainError::InvalidData(
            "Skill name must use lowercase ASCII letters, digits, '-' or '_'".to_string(),
        ));
    }
    Ok(value.to_string())
}

pub(super) fn validate_skill_scope(scope: &SkillScope) -> Result<(), DomainError> {
    match scope {
        SkillScope::Global => Ok(()),
        SkillScope::Preset { api_id, name } => {
            validate_scope_text(api_id, "preset api_id")?;
            validate_scope_text(name, "preset name")
        }
        SkillScope::Profile { profile_id } => validate_profile_scope_id(profile_id),
        SkillScope::Character { character_id } => validate_scope_text(character_id, "character id"),
    }
}

pub(super) fn validate_skill_scope_filter(filter: &SkillScopeFilter) -> Result<(), DomainError> {
    match filter {
        SkillScopeFilter::All => Ok(()),
        SkillScopeFilter::Global => validate_skill_scope(&SkillScope::Global),
        SkillScopeFilter::Preset { api_id, name } => validate_skill_scope(&SkillScope::Preset {
            api_id: api_id.clone(),
            name: name.clone(),
        }),
        SkillScopeFilter::Profile { profile_id } => validate_skill_scope(&SkillScope::Profile {
            profile_id: profile_id.clone(),
        }),
        SkillScopeFilter::Character { character_id } => {
            validate_skill_scope(&SkillScope::Character {
                character_id: character_id.clone(),
            })
        }
    }
}

pub(super) fn skill_scope_storage_dir(scope: &SkillScope) -> Result<String, DomainError> {
    validate_skill_scope(scope)?;
    Ok(match scope {
        SkillScope::Global => "global".to_string(),
        SkillScope::Preset { .. } => format!("preset_{}", scope_digest(scope)),
        SkillScope::Profile { .. } => format!("profile_{}", scope_digest(scope)),
        SkillScope::Character { .. } => format!("character_{}", scope_digest(scope)),
    })
}

fn scope_digest(scope: &SkillScope) -> String {
    let digest = Sha256::digest(scope.stable_key().as_bytes());
    let mut output = String::with_capacity(16);
    for byte in &digest[..8] {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn validate_scope_text(raw: &str, label: &str) -> Result<(), DomainError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(format!("{label} cannot be empty")));
    }
    if value.contains('\0') {
        return Err(DomainError::InvalidData(format!(
            "{label} cannot contain NUL"
        )));
    }
    Ok(())
}

fn validate_profile_scope_id(raw: &str) -> Result<(), DomainError> {
    validate_scope_text(raw, "profile id")?;
    if raw.len() > 128 {
        return Err(DomainError::InvalidData(
            "profile id must be <= 128 characters".to_string(),
        ));
    }
    if !raw.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    }) {
        return Err(DomainError::InvalidData(
            "profile id must use lowercase ASCII letters, digits, '-' or '_'".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn normalize_skill_path(raw: &str) -> Result<String, DomainError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(
            "Skill file path cannot be empty".to_string(),
        ));
    }
    if value.contains('\0') {
        return Err(DomainError::InvalidData(
            "Skill file path cannot contain NUL".to_string(),
        ));
    }
    if value.starts_with('/') || value.starts_with('\\') {
        return Err(DomainError::InvalidData(
            "Skill file path must be relative".to_string(),
        ));
    }
    if value.len() >= 2 && value.as_bytes()[1] == b':' && value.as_bytes()[0].is_ascii_alphabetic()
    {
        return Err(DomainError::InvalidData(
            "Skill file path cannot use a Windows drive prefix".to_string(),
        ));
    }

    let normalized = value.replace('\\', "/");
    let mut parts = Vec::new();
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err(DomainError::InvalidData(
                "Skill file path cannot contain ..".to_string(),
            ));
        }
        if matches!(segment, ".git" | ".ssh" | ".env") {
            return Err(DomainError::InvalidData(format!(
                "Skill file path contains forbidden segment: {segment}"
            )));
        }
        parts.push(segment);
    }
    if parts.is_empty() {
        return Err(DomainError::InvalidData(
            "Skill file path cannot be empty".to_string(),
        ));
    }
    Ok(parts.join("/"))
}

pub(super) fn normalize_source_string(raw: &str, label: &str) -> Result<String, DomainError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(format!("{label} cannot be empty")));
    }
    if value.contains('\0') {
        return Err(DomainError::InvalidData(format!(
            "{label} cannot contain NUL"
        )));
    }
    Ok(value.to_string())
}

pub(super) fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

const HEX: &[u8; 16] = b"0123456789abcdef";
