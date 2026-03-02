use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{LanSyncDiffPlan, LanSyncManifest, LanSyncManifestEntry};
use crate::infrastructure::lan_sync::paths::{
    is_excluded_relative_path, normalize_relative_path, sync_scope_directories, sync_scope_files,
};

pub async fn scan_manifest(data_root: PathBuf) -> Result<LanSyncManifest, DomainError> {
    tokio::task::spawn_blocking(move || scan_manifest_sync(&data_root))
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
}

fn scan_manifest_sync(data_root: &Path) -> Result<LanSyncManifest, DomainError> {
    let mut entries = Vec::new();

    for directory in sync_scope_directories() {
        let root = data_root.join(directory);
        if !root.exists() {
            continue;
        }
        if !root.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Sync scope root is not a directory: {}",
                root.display()
            )));
        }

        scan_dir_recursive(data_root, &root, &mut entries)?;
    }

    for file in sync_scope_files() {
        let path = data_root.join(file);
        if !path.exists() {
            continue;
        }
        if !path.is_file() {
            return Err(DomainError::InvalidData(format!(
                "Sync scope root is not a file: {}",
                path.display()
            )));
        }

        entries.push(make_entry(data_root, &path)?);
    }

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(LanSyncManifest { entries })
}

fn scan_dir_recursive(
    data_root: &Path,
    dir: &Path,
    entries: &mut Vec<LanSyncManifestEntry>,
) -> Result<(), DomainError> {
    for entry in std::fs::read_dir(dir).map_err(|error| {
        DomainError::InternalError(format!("Failed to read directory {}: {}", dir.display(), error))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read directory entry in {}: {}",
                dir.display(),
                error
            ))
        })?;

        let file_type = entry.file_type().map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read file type for {}: {}",
                entry.path().display(),
                error
            ))
        })?;

        let entry_path = entry.path();

        if file_type.is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Symlinks are not supported in sync scope: {}",
                entry_path.display()
            )));
        }

        if file_type.is_dir() {
            let relative = entry_path.strip_prefix(data_root)
                .map_err(|error| DomainError::InternalError(error.to_string()))?;
            let relative = normalize_relative_path(relative)?;

            if is_excluded_relative_path(&relative) {
                continue;
            }

            scan_dir_recursive(data_root, &entry_path, entries)?;
            continue;
        }

        if file_type.is_file() {
            let relative = entry_path.strip_prefix(data_root)
                .map_err(|error| DomainError::InternalError(error.to_string()))?;
            let relative = normalize_relative_path(relative)?;

            if is_excluded_relative_path(&relative) {
                continue;
            }

            entries.push(make_entry(data_root, &entry_path)?);
        }
    }

    Ok(())
}

fn make_entry(data_root: &Path, file_path: &Path) -> Result<LanSyncManifestEntry, DomainError> {
    let relative = file_path
        .strip_prefix(data_root)
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let relative_path = normalize_relative_path(relative)?;

    let metadata = std::fs::metadata(file_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read metadata for {}: {}",
            file_path.display(),
            error
        ))
    })?;

    let modified_ms = metadata
        .modified()
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .as_millis() as u64;

    Ok(LanSyncManifestEntry {
        relative_path,
        size_bytes: metadata.len(),
        modified_ms,
    })
}

pub fn diff_manifests(source: &LanSyncManifest, target: &LanSyncManifest) -> LanSyncDiffPlan {
    let target_index: HashMap<&str, (u64, u64)> = target
        .entries
        .iter()
        .map(|entry| (entry.relative_path.as_str(), (entry.size_bytes, entry.modified_ms)))
        .collect();

    let mut download = Vec::new();
    let mut bytes_total = 0u64;
    for entry in &source.entries {
        let is_same = target_index
            .get(entry.relative_path.as_str())
            .is_some_and(|value| *value == (entry.size_bytes, entry.modified_ms));

        if is_same {
            continue;
        }

        bytes_total += entry.size_bytes;
        download.push(entry.clone());
    }

    let files_total = download.len();

    LanSyncDiffPlan {
        download,
        files_total,
        bytes_total,
    }
}
