use std::path::{Component, Path, PathBuf};

use crate::domain::errors::DomainError;

const SYNC_SCOPE_DIRECTORIES: &[&str] = &[
    "chats",
    "characters",
    "groups",
    "group chats",
    "worlds",
    "themes",
    "user",
    "User Avatars",
    "OpenAI Settings",
];

const SYNC_SCOPE_FILES: &[&str] = &["settings.json"];

pub fn sync_scope_directories() -> &'static [&'static str] {
    SYNC_SCOPE_DIRECTORIES
}

pub fn sync_scope_files() -> &'static [&'static str] {
    SYNC_SCOPE_FILES
}

pub fn is_excluded_relative_path(relative_path: &str) -> bool {
    let value = relative_path.trim();
    value == "user/lan-sync" || value.starts_with("user/lan-sync/")
}

pub fn validate_relative_path(relative_path: &str) -> Result<(), DomainError> {
    let value = relative_path.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(
            "Relative path is empty".to_string(),
        ));
    }

    if value.starts_with('/') {
        return Err(DomainError::InvalidData(
            "Relative path must not start with '/'".to_string(),
        ));
    }

    if value.contains('\\') {
        return Err(DomainError::InvalidData(
            "Relative path must use '/' separators".to_string(),
        ));
    }

    if is_excluded_relative_path(value) {
        return Err(DomainError::InvalidData(format!(
            "Path is excluded from sync scope: {}",
            value
        )));
    }

    let parts: Vec<&str> = value.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(DomainError::InvalidData(format!(
            "Relative path contains invalid components: {}",
            value
        )));
    }

    if parts.len() == 1 {
        if SYNC_SCOPE_FILES.contains(&parts[0]) {
            return Ok(());
        }

        return Err(DomainError::InvalidData(format!(
            "Path not allowed in sync scope: {}",
            value
        )));
    }

    if !SYNC_SCOPE_DIRECTORIES.contains(&parts[0]) {
        return Err(DomainError::InvalidData(format!(
            "Path not allowed in sync scope: {}",
            value
        )));
    }

    Ok(())
}

pub fn resolve_relative_path(
    data_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, DomainError> {
    validate_relative_path(relative_path)?;

    let mut full_path = PathBuf::from(data_root);
    for part in relative_path.split('/') {
        full_path.push(part);
    }

    Ok(full_path)
}

pub fn normalize_relative_path(path: &Path) -> Result<String, DomainError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_str().ok_or_else(|| {
                DomainError::InvalidData("Path contains non-UTF-8 components".to_string())
            })?),
            Component::CurDir => continue,
            other => {
                return Err(DomainError::InvalidData(format!(
                    "Path contains unsupported component: {:?}",
                    other
                )));
            }
        }
    }

    Ok(parts.join("/"))
}
