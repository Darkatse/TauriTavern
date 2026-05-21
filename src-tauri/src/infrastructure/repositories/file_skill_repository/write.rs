use std::fs;
use std::path::Path;

use serde_json::Value;
use uuid::Uuid;

use super::fs_ops::{
    cleanup_dir, copy_dir_contents, prepare_skill_dir_replacement,
    rollback_prepared_skill_dir_replacement,
};
use super::index::sort_index;
use super::package::{sha256_hex, validate_skill_root};
use super::paths::{normalize_skill_path, validate_skill_name, validate_skill_scope};
use super::{FileSkillRepository, MAX_READ_CHARS, MAX_SINGLE_FILE_BYTES, MAX_SKILL_MD_BYTES};
use crate::domain::errors::DomainError;
use crate::domain::models::skill::{SkillReadRequest, SkillReadResult, SkillWriteRequest};

pub(super) async fn write_skill_file(
    repository: &FileSkillRepository,
    request: SkillWriteRequest,
) -> Result<SkillReadResult, DomainError> {
    let staging_dir = repository
        .staging_root()
        .join(format!("write-{}", Uuid::new_v4().simple()));
    let result = write_skill_file_inner(repository, request, &staging_dir).await;
    cleanup_dir(&staging_dir);
    result
}

async fn write_skill_file_inner(
    repository: &FileSkillRepository,
    request: SkillWriteRequest,
    staging_dir: &Path,
) -> Result<SkillReadResult, DomainError> {
    let name = validate_skill_name(&request.name)?;
    validate_skill_scope(&request.scope)?;
    let path = normalize_skill_path(&request.path)?;
    validate_write_size(&path, request.content.as_bytes().len() as u64)?;

    let target_root = repository
        .installed_skill_root(&request.scope, &name)
        .await?;
    let target_file = target_root.join(&path);
    ensure_existing_text_file(&target_file, &path, request.expected_sha256.as_deref())?;

    let mut index = repository.load_index().await?;
    let position = index
        .skills
        .iter()
        .position(|skill| skill.scope == request.scope && skill.name == name)
        .ok_or_else(|| {
            DomainError::NotFound(format!(
                "Skill not found: {}/{}",
                request.scope.label(),
                name
            ))
        })?;
    let existing_entry = index.skills[position].clone();

    let package_root = staging_dir.join("package");
    copy_dir_contents(&target_root, &package_root)?;
    let staged_file = package_root.join(&path);
    fs::write(&staged_file, request.content.as_bytes()).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write staged Skill file '{}': {}",
            staged_file.display(),
            error
        ))
    })?;

    let mut validated = validate_skill_root(&package_root, Value::Null)?.entry;
    if validated.name != name {
        return Err(DomainError::InvalidData(format!(
            "Skill file edit cannot rename '{}' to '{}'",
            name, validated.name
        )));
    }
    validated.scope = request.scope.clone();
    validated.installed_at = existing_entry.installed_at;
    validated.source_refs = existing_entry.source_refs;

    index.skills[position] = validated;
    sort_index(&mut index);

    let replacement = prepare_skill_dir_replacement(&package_root, &target_root, &name)?;
    if let Err(error) = repository.save_index(&index).await {
        return Err(rollback_prepared_skill_dir_replacement(&replacement, error));
    }
    if let Err(error) = replacement.discard_backup() {
        return Err(DomainError::InternalError(format!(
            "write_skill_file committed but failed to clean up Skill directories: {error}"
        )));
    }

    super::read::read_skill_file(
        repository,
        SkillReadRequest {
            scope: request.scope,
            name,
            path,
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: Some(MAX_READ_CHARS),
        },
    )
    .await
}

fn validate_write_size(path: &str, bytes: u64) -> Result<(), DomainError> {
    if bytes > MAX_SINGLE_FILE_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Skill file '{}' exceeds {} bytes",
            path, MAX_SINGLE_FILE_BYTES
        )));
    }
    if path == "SKILL.md" && bytes > MAX_SKILL_MD_BYTES {
        return Err(DomainError::InvalidData(format!(
            "SKILL.md must be <= {MAX_SKILL_MD_BYTES} bytes"
        )));
    }
    Ok(())
}

fn ensure_existing_text_file(
    path: &Path,
    skill_path: &str,
    expected_sha256: Option<&str>,
) -> Result<(), DomainError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Skill file not found: {skill_path}"))
        } else {
            DomainError::InternalError(format!(
                "Failed to read Skill file metadata '{}': {}",
                path.display(),
                error
            ))
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Skill file cannot be a symlink: {skill_path}"
        )));
    }
    if !metadata.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Skill path is not a file: {skill_path}"
        )));
    }

    let bytes = fs::read(path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill file '{}': {}",
            path.display(),
            error
        ))
    })?;
    if std::str::from_utf8(&bytes).is_err() {
        return Err(DomainError::InvalidData(format!(
            "Cannot edit binary Skill file: {skill_path}"
        )));
    }

    let expected = expected_sha256
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(expected) = expected {
        let actual = sha256_hex(&bytes);
        if expected != actual {
            return Err(DomainError::InvalidData(format!(
                "Skill file changed on disk: {skill_path}"
            )));
        }
    }
    Ok(())
}
