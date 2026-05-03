use crate::domain::errors::DomainError;

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
