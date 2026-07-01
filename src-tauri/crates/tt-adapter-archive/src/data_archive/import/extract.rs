use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;

use super::archive::{ArchiveReadEntry, StagedArchive, StagedEntry, ZipImportArchive};
use super::layout::{ArchiveLayoutPolicy, DetectedArchiveLayout};
use crate::data_archive::shared::{
    COPY_BUFFER_BYTES, IMPORT_TARGET_USER_HANDLE, PROGRESS_REPORT_MIN_DELTA,
    components_after_prefix, copy_stream_with_cancel, create_output_file_replacing_directory,
    ensure_not_cancelled, ensure_output_directory, internal_error, is_macos_resource_fork_path,
    progress_percent,
};

pub fn extract_zip_to_normalized_root(
    archive: &mut ZipImportArchive,
    layout: &DetectedArchiveLayout,
    normalized_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    let total_entries = layout.scanned_entries.max(1) as u64;
    let mut processed_entries = 0u64;
    let mut last_reported_percent = 0.0f32;
    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut last_ensured_parent: Option<PathBuf> = None;
    let detected_user_handles = layout.detected_user_handles().clone();

    archive.read_entries(is_cancelled, &mut |mut archive_entry| {
        ensure_not_cancelled(is_cancelled)?;

        let sanitized_path = archive_entry.path().to_path_buf();
        processed_entries = processed_entries.saturating_add(1);

        let Some(target_relative_path) =
            target_relative_path(&sanitized_path, layout, &detected_user_handles)
        else {
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            return Ok(());
        };
        let output_path = normalized_root.join(target_relative_path);

        if archive_entry.is_dir() {
            ensure_output_directory(&output_path)?;
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            return Ok(());
        }

        if let Some(parent) = output_path.parent() {
            let should_create_parent = last_ensured_parent
                .as_ref()
                .map(|last| last != parent)
                .unwrap_or(true);
            if should_create_parent {
                fs::create_dir_all(parent).map_err(|error| {
                    internal_error("Failed to create normalized parent directory", error)
                })?;
                last_ensured_parent = Some(parent.to_path_buf());
            }
        }

        let mut output_file = create_output_file_replacing_directory(&output_path)?;
        let ArchiveReadEntry::File { reader, .. } = &mut archive_entry else {
            return Err(DomainError::InternalError(
                "Archive entry reader is missing".to_string(),
            ));
        };
        copy_stream_with_cancel(
            reader,
            &mut output_file,
            &mut copy_buffer,
            is_cancelled,
            "Failed to read archive entry data",
            "Failed to write normalized output file",
        )?;

        maybe_report_extraction_progress(
            processed_entries,
            total_entries,
            &mut last_reported_percent,
            report_progress,
        );

        Ok(())
    })
}

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

fn target_relative_path(
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
            if let Some(first) = relative_components.first() {
                if detected_user_handles.contains(first) {
                    let mut target = PathBuf::from(IMPORT_TARGET_USER_HANDLE);
                    for component in relative_components.iter().skip(1) {
                        target.push(component);
                    }
                    return target;
                }
            }

            let mut target = PathBuf::new();
            for component in relative_components {
                target.push(component);
            }
            target
        }
    }
}

fn move_staged_file_into_place(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), DomainError> {
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
        fs::remove_file(target_path).map_err(|error| {
            internal_error("Failed to replace staged archive file", error)
        })?;
    }

    fs::rename(source_path, target_path)
        .map_err(|error| internal_error("Failed to move staged archive file", error))
}

fn maybe_report_extraction_progress(
    processed_entries: u64,
    total_entries: u64,
    last_reported_percent: &mut f32,
    report_progress: &mut dyn FnMut(&str, f32, &str),
) {
    let percent = progress_percent(processed_entries, total_entries, 15.0, 90.0);
    let should_report = processed_entries >= total_entries
        || percent - *last_reported_percent >= PROGRESS_REPORT_MIN_DELTA;
    if !should_report {
        return;
    }

    *last_reported_percent = percent;
    report_progress("extracting", percent, "Extracting and normalizing archive");
}
