use std::fs;
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;
use uuid::Uuid;

pub(super) fn create_temp_directory(parent: &Path, prefix: &str) -> Result<PathBuf, DomainError> {
    let candidate = parent.join(format!(".{prefix}-{}", Uuid::new_v4().simple()));
    fs::create_dir(&candidate).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create extension staging directory '{}': {error}",
            candidate.display()
        ))
    })?;
    Ok(candidate)
}

pub(super) fn cleanup_temp_directory(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

pub(super) fn replace_directory(source: &Path, destination: &Path) -> Result<(), DomainError> {
    let backup_path =
        destination.with_file_name(format!(".tmp-extension-backup-{}", Uuid::new_v4().simple()));

    fs::rename(destination, &backup_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to stage existing extension '{}' for replacement: {error}",
            destination.display()
        ))
    })?;

    if let Err(error) = fs::rename(source, destination) {
        let _ = fs::rename(&backup_path, destination);
        return Err(DomainError::InternalError(format!(
            "Failed to activate extension '{}': {error}",
            destination.display()
        )));
    }

    if let Err(error) = fs::remove_dir_all(&backup_path) {
        tracing::warn!(
            "Failed to remove extension backup directory '{}': {error}",
            backup_path.display()
        );
    }

    Ok(())
}
