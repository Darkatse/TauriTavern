use std::fs;
use std::path::Path;

use uuid::Uuid;

use super::MAX_SINGLE_FILE_BYTES;
use super::paths::normalize_skill_path;
use crate::domain::errors::DomainError;

pub(super) fn copy_dir_contents(source: &Path, destination: &Path) -> Result<(), DomainError> {
    fs::create_dir_all(destination).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create Skill package directory '{}': {}",
            destination.display(),
            error
        ))
    })?;

    for entry in fs::read_dir(source).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read Skill package directory '{}': {}",
            source.display(),
            error
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!("Failed to read Skill package entry: {error}"))
        })?;
        let source_path = entry.path();
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read Skill package entry metadata '{}': {}",
                source_path.display(),
                error
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Skill package cannot contain symlink: {}",
                source_path.display()
            )));
        }
        let relative = source_path.strip_prefix(source).map_err(|error| {
            DomainError::InternalError(format!("Failed to compute Skill relative path: {error}"))
        })?;
        let normalized = normalize_skill_path(&relative.to_string_lossy())?;
        let target_path = destination.join(normalized);
        if metadata.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else if metadata.is_file() {
            if metadata.len() > MAX_SINGLE_FILE_BYTES {
                return Err(DomainError::InvalidData(format!(
                    "Skill file '{}' exceeds {} bytes",
                    source_path.display(),
                    MAX_SINGLE_FILE_BYTES
                )));
            }
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create Skill package parent '{}': {}",
                        parent.display(),
                        error
                    ))
                })?;
            }
            fs::copy(&source_path, &target_path).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to copy Skill package file '{}' -> '{}': {}",
                    source_path.display(),
                    target_path.display(),
                    error
                ))
            })?;
        }
    }
    Ok(())
}

pub(super) fn activate_package_dir(source: &Path, target: &Path) -> Result<(), DomainError> {
    let backup = target.with_file_name(format!(
        ".backup-{}-{}",
        target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("skill"),
        Uuid::new_v4().simple()
    ));
    let had_target = target.exists();
    if had_target {
        fs::rename(target, &backup).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to move existing Skill '{}' to backup '{}': {}",
                target.display(),
                backup.display(),
                error
            ))
        })?;
    }

    let result = fs::rename(source, target).or_else(|rename_error| {
        copy_dir_contents(source, target)?;
        fs::remove_dir_all(source).map_err(|remove_error| {
            DomainError::InternalError(format!(
                "Failed to remove Skill staging directory after copy fallback. Rename error: {}; remove error: {}",
                rename_error, remove_error
            ))
        })
    });

    match result {
        Ok(()) => {
            if had_target {
                cleanup_dir(&backup);
            }
            Ok(())
        }
        Err(error) => {
            if had_target {
                let _ = fs::rename(&backup, target);
            }
            Err(error)
        }
    }
}

pub(super) fn delete_installed_skill_dir(path: &Path, name: &str) -> Result<(), DomainError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Skill directory not found: {name}"))
        } else {
            DomainError::InternalError(format!(
                "Failed to read Skill directory metadata '{}': {}",
                path.display(),
                error
            ))
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Skill directory cannot be a symlink: {name}"
        )));
    }
    if !metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Skill installed path is not a directory: {name}"
        )));
    }

    fs::remove_dir_all(path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to delete Skill directory '{}': {}",
            path.display(),
            error
        ))
    })
}

pub(super) fn cleanup_dir(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}
