use std::collections::BTreeSet;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::domain::errors::DomainError;
use crate::infrastructure::zipkit;

use super::layout::{LayoutKind, LayoutMeta};
use crate::infrastructure::persistence::data_archive::shared::{
    COPY_BUFFER_BYTES, FILE_IO_BUFFER_BYTES, PROGRESS_REPORT_MIN_DELTA, components_after_prefix,
    copy_stream_with_cancel, create_output_file_replacing_directory, ensure_not_cancelled,
    ensure_output_directory, internal_error, progress_percent,
};

pub fn extract_to_normalized_root_streaming(
    archive_path: &Path,
    layout: &LayoutMeta,
    normalized_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    let mut archive = ZipArchive::new(archive_reader)
        .map_err(|error| internal_error("Failed to parse archive file", error))?;

    let total_entries = layout.scanned_entries.max(1) as u64;
    let mut processed_entries = 0u64;
    let mut last_reported_percent = 0.0f32;
    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut last_ensured_parent: Option<PathBuf> = None;
    let source_users_lookup = layout
        .source_users()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    for index in 0..archive.len() {
        ensure_not_cancelled(is_cancelled)?;

        let mut archive_entry = archive
            .by_index(index)
            .map_err(|error| internal_error("Failed to read archive entry", error))?;
        let sanitized_path = zipkit::enclosed_zip_entry_path(&archive_entry)?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        processed_entries = processed_entries.saturating_add(1);

        let Some(rel_components) = components_after_prefix(&sanitized_path, &layout.source_prefix)
        else {
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            continue;
        };
        if rel_components.is_empty() {
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            continue;
        }

        let target_relative_path =
            map_to_normalized_path(&rel_components, layout.kind, &source_users_lookup);
        let output_path = normalized_root.join(target_relative_path);

        if archive_entry.is_dir() {
            ensure_output_directory(&output_path)?;
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            continue;
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
        copy_stream_with_cancel(
            &mut archive_entry,
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
    }

    Ok(())
}

fn map_to_normalized_path(
    relative_components: &[String],
    kind: LayoutKind,
    source_users: &BTreeSet<String>,
) -> PathBuf {
    match kind {
        LayoutKind::UserRoot => {
            let mut target = PathBuf::from(
                crate::infrastructure::persistence::data_archive::shared::DEFAULT_USER_HANDLE,
            );
            for component in relative_components {
                target.push(component);
            }
            target
        }
        LayoutKind::DataRoot | LayoutKind::UserHandleRoot => {
            if let Some(first) = relative_components.first() {
                if source_users.contains(first) {
                    let mut target = PathBuf::from(
                        crate::infrastructure::persistence::data_archive::shared::DEFAULT_USER_HANDLE,
                    );
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
