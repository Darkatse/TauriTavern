use std::fs;

use super::paths::{normalize_skill_path, validate_skill_name};
use super::{DEFAULT_READ_MAX_CHARS, FileSkillRepository, MAX_READ_CHARS};
use crate::domain::errors::DomainError;
use crate::domain::models::skill::SkillReadResult;

pub(super) async fn read_skill_file(
    repository: &FileSkillRepository,
    name: &str,
    path: &str,
    max_chars: Option<usize>,
) -> Result<SkillReadResult, DomainError> {
    let name = validate_skill_name(name)?;
    let path = normalize_skill_path(path)?;
    let index = repository.load_index().await?;
    if !index.skills.iter().any(|skill| skill.name == name) {
        return Err(DomainError::NotFound(format!("Skill not found: {name}")));
    }

    let skill_root = repository.installed_root().join(&name);
    let root_metadata = fs::symlink_metadata(&skill_root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Skill directory not found: {name}"))
        } else {
            DomainError::InternalError(format!(
                "Failed to read Skill directory metadata '{}': {}",
                skill_root.display(),
                error
            ))
        }
    })?;
    if root_metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Skill directory cannot be a symlink: {name}"
        )));
    }
    if !root_metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Skill installed path is not a directory: {name}"
        )));
    }

    let full_path = skill_root.join(&path);
    let metadata = fs::symlink_metadata(&full_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Skill file not found: skills/{name}/{path}"))
        } else {
            DomainError::InternalError(format!(
                "Failed to read Skill file metadata '{}': {}",
                full_path.display(),
                error
            ))
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Skill file cannot be a symlink: skills/{name}/{path}"
        )));
    }
    if !metadata.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Skill path is not a file: skills/{name}/{path}"
        )));
    }

    let canonical_root = fs::canonicalize(&skill_root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to resolve Skill directory '{}': {}",
            skill_root.display(),
            error
        ))
    })?;
    let canonical_file = fs::canonicalize(&full_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to resolve Skill file '{}': {}",
            full_path.display(),
            error
        ))
    })?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(DomainError::InvalidData(format!(
            "Skill file escapes installed directory: skills/{name}/{path}"
        )));
    }

    let bytes = fs::read(&full_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill file '{}': {}",
            full_path.display(),
            error
        ))
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        DomainError::InvalidData(format!(
            "Skill file is not UTF-8 text: skills/{name}/{path}"
        ))
    })?;
    let requested_chars = max_chars.unwrap_or(DEFAULT_READ_MAX_CHARS);
    if requested_chars == 0 {
        return Err(DomainError::InvalidData(
            "max_chars must be greater than 0".to_string(),
        ));
    }
    if requested_chars > MAX_READ_CHARS {
        return Err(DomainError::InvalidData(format!(
            "max_chars must be <= {MAX_READ_CHARS}"
        )));
    }

    let total_chars = text.chars().count();
    let truncated = total_chars > requested_chars;
    let content = if truncated {
        text.chars().take(requested_chars).collect()
    } else {
        text.to_string()
    };
    let sha256 = super::package::sha256_hex(&bytes);

    Ok(SkillReadResult {
        name: name.clone(),
        path: path.clone(),
        content,
        chars: total_chars.min(requested_chars),
        bytes: bytes.len() as u64,
        sha256,
        truncated,
        resource_ref: format!("skills/{name}/{path}"),
    })
}
