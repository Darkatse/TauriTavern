use std::fs;
use std::path::{Path, PathBuf};

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

pub(super) fn copy_skill_dir_to_empty_target(
    source: &Path,
    target: &Path,
    name: &str,
) -> Result<(), DomainError> {
    if target.exists() {
        return Err(DomainError::InvalidData(format!(
            "Skill target directory already exists without an index entry: {}",
            target.display()
        )));
    }

    if let Err(error) = copy_dir_contents(source, target) {
        return Err(cleanup_after_copy_error(target, name, error));
    }
    Ok(())
}

pub(super) struct PreparedSkillDirReplacement {
    target: PathBuf,
    backup: PathBuf,
}

impl PreparedSkillDirReplacement {
    pub(super) fn rollback(&self) -> Result<(), DomainError> {
        remove_dir_if_exists(&self.target)?;
        fs::rename(&self.backup, &self.target).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to restore Skill directory backup '{}' -> '{}': {}",
                self.backup.display(),
                self.target.display(),
                error
            ))
        })
    }

    pub(super) fn discard_backup(&self) -> Result<(), DomainError> {
        remove_dir_if_exists(&self.backup)
    }
}

pub(super) fn prepare_skill_dir_replacement(
    source: &Path,
    target: &Path,
    name: &str,
) -> Result<PreparedSkillDirReplacement, DomainError> {
    ensure_installed_skill_dir(target, name)?;
    let backup = target.with_file_name(format!(
        ".backup-{}-{}",
        target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("skill"),
        Uuid::new_v4().simple()
    ));

    fs::rename(target, &backup).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to move existing Skill '{}' to backup '{}': {}",
            target.display(),
            backup.display(),
            error
        ))
    })?;

    if let Err(error) = copy_dir_contents(source, target) {
        let cleanup_error = cleanup_after_copy_error(target, name, error);
        return Err(match fs::rename(&backup, target) {
            Ok(()) => cleanup_error,
            Err(restore_error) => DomainError::InternalError(format!(
                "{}; additionally failed to restore Skill directory backup '{}' -> '{}': {}",
                cleanup_error,
                backup.display(),
                target.display(),
                restore_error
            )),
        });
    }

    Ok(PreparedSkillDirReplacement {
        target: target.to_path_buf(),
        backup,
    })
}

pub(super) fn ensure_installed_skill_dir(path: &Path, name: &str) -> Result<(), DomainError> {
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

    Ok(())
}

pub(super) fn delete_installed_skill_dir(path: &Path, name: &str) -> Result<(), DomainError> {
    ensure_installed_skill_dir(path, name)?;
    fs::remove_dir_all(path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to delete Skill directory '{}': {}",
            path.display(),
            error
        ))
    })
}

pub(super) fn remove_dir_if_exists(path: &Path) -> Result<(), DomainError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to remove Skill directory '{}': {}",
            path.display(),
            error
        ))),
    }
}

pub(super) fn rollback_prepared_skill_dir(target: &Path, error: DomainError) -> DomainError {
    match remove_dir_if_exists(target) {
        Ok(()) => error,
        Err(cleanup_error) => DomainError::InternalError(format!(
            "{}; additionally failed to roll back prepared Skill directory '{}': {}",
            error,
            target.display(),
            cleanup_error
        )),
    }
}

pub(super) fn rollback_prepared_skill_dirs(targets: &[PathBuf], error: DomainError) -> DomainError {
    let mut cleanup_errors = Vec::new();
    for target in targets {
        if let Err(cleanup_error) = remove_dir_if_exists(target) {
            cleanup_errors.push(format!("{}: {}", target.display(), cleanup_error));
        }
    }

    if cleanup_errors.is_empty() {
        error
    } else {
        DomainError::InternalError(format!(
            "{}; additionally failed to roll back prepared Skill directories: {}",
            error,
            cleanup_errors.join("; ")
        ))
    }
}

pub(super) fn rollback_prepared_skill_dir_replacement(
    replacement: &PreparedSkillDirReplacement,
    error: DomainError,
) -> DomainError {
    match replacement.rollback() {
        Ok(()) => error,
        Err(rollback_error) => DomainError::InternalError(format!(
            "{}; additionally failed to roll back replaced Skill directory: {}",
            error, rollback_error
        )),
    }
}

pub(super) fn cleanup_dir(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}

fn cleanup_after_copy_error(target: &Path, name: &str, error: DomainError) -> DomainError {
    match remove_dir_if_exists(target) {
        Ok(()) => error,
        Err(cleanup_error) => DomainError::InternalError(format!(
            "{}; additionally failed to clean up prepared Skill directory for '{}': {}",
            error, name, cleanup_error
        )),
    }
}
