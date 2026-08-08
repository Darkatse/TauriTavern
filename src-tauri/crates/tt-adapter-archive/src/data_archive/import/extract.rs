use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;

use super::archive::{StagedArchive, StagedEntry};
use super::layout::{ArchiveLayoutPolicy, DetectedArchiveLayout};
use crate::data_archive::shared::{
    IMPORT_TARGET_USER_HANDLE, components_after_prefix, ensure_not_cancelled,
    ensure_output_directory, internal_error, is_macos_resource_fork_path,
};

pub fn normalize_staged_archive(
    archive: &StagedArchive,
    layout: &DetectedArchiveLayout,
    normalized_root: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    let detected_user_handles = layout.detected_user_handles().clone();
    for entry in archive.entries() {
        ensure_not_cancelled(is_cancelled)?;

        let Some(target_relative_path) =
            target_relative_path(entry.path(), layout, &detected_user_handles)
        else {
            continue;
        };
        let target_path = normalized_root.join(target_relative_path);

        match entry {
            StagedEntry::Directory { .. } => ensure_output_directory(&target_path)?,
            StagedEntry::File { payload_path, .. } => {
                move_staged_file_into_place(payload_path, &target_path)?;
            }
        }
    }

    Ok(())
}

pub(super) fn target_relative_path(
    sanitized_path: &Path,
    layout: &DetectedArchiveLayout,
    detected_user_handles: &BTreeSet<String>,
) -> Option<PathBuf> {
    if is_macos_resource_fork_path(sanitized_path) {
        return None;
    }

    let rel_components = components_after_prefix(sanitized_path, &layout.archive_root_prefix)?;
    if rel_components.is_empty() {
        return None;
    }

    Some(map_archive_entry_to_data_root_path(
        &rel_components,
        layout.policy,
        detected_user_handles,
    ))
}

fn map_archive_entry_to_data_root_path(
    relative_components: &[String],
    policy: ArchiveLayoutPolicy,
    detected_user_handles: &BTreeSet<String>,
) -> PathBuf {
    match policy {
        ArchiveLayoutPolicy::SillyTavernUserRoot => {
            let mut target = PathBuf::from(IMPORT_TARGET_USER_HANDLE);
            for component in relative_components {
                target.push(component);
            }
            target
        }
        ArchiveLayoutPolicy::DataRoot | ArchiveLayoutPolicy::UserHandleRoot => {
            if let Some(first) = relative_components.first()
                && detected_user_handles.contains(first)
            {
                let mut target = PathBuf::from(IMPORT_TARGET_USER_HANDLE);
                for component in relative_components.iter().skip(1) {
                    target.push(component);
                }
                return target;
            }

            let mut target = PathBuf::new();
            for component in relative_components {
                target.push(component);
            }
            target
        }
    }
}

fn move_staged_file_into_place(source_path: &Path, target_path: &Path) -> Result<(), DomainError> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            internal_error("Failed to create normalized parent directory", error)
        })?;
    }

    if target_path.is_dir() {
        fs::remove_dir_all(target_path).map_err(|error| {
            internal_error(
                "Failed to replace directory with staged archive file",
                error,
            )
        })?;
    } else if target_path.exists() {
        fs::remove_file(target_path)
            .map_err(|error| internal_error("Failed to replace staged archive file", error))?;
    }

    fs::rename(source_path, target_path)
        .map_err(|error| internal_error("Failed to move staged archive file", error))
}
