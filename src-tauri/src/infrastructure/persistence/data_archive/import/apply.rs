use std::fs;
use std::fs::File;
use std::path::Path;

use crate::domain::errors::DomainError;
use crate::domain::models::data_archive::{
    DataArchiveImportFailure, DataArchiveLocalMutationSummary,
};

use crate::infrastructure::persistence::data_archive::shared::{
    COPY_BUFFER_BYTES, copy_stream_with_cancel, ensure_not_cancelled, internal_error,
    read_directory_sorted,
};

pub fn apply_overlay(
    normalized_root: &Path,
    data_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DataArchiveLocalMutationSummary, DataArchiveImportFailure> {
    let mut local_applied = DataArchiveLocalMutationSummary::default();
    if !data_root.exists() {
        fs::create_dir_all(data_root).map_err(|error| {
            DataArchiveImportFailure::new(
                internal_error(
                    "Failed to create data root directory before applying overlay",
                    error,
                ),
                local_applied,
            )
        })?;
        local_applied.mark_target_changed();
    }

    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    if let Err(error) = apply_directory_recursive(
        normalized_root,
        normalized_root,
        data_root,
        &mut copy_buffer,
        &mut local_applied,
        is_cancelled,
    ) {
        return Err(DataArchiveImportFailure::new(error, local_applied));
    }

    report_progress("applying", 99.0, "Merge completed");
    Ok(local_applied)
}

fn apply_directory_recursive(
    normalized_root: &Path,
    current: &Path,
    data_root: &Path,
    copy_buffer: &mut [u8],
    local_applied: &mut DataArchiveLocalMutationSummary,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    for entry in read_directory_sorted(current)? {
        ensure_not_cancelled(is_cancelled)?;

        let source_path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read normalized entry type", error))?;
        let relative_path = source_path
            .strip_prefix(normalized_root)
            .map_err(|error| internal_error("Failed to resolve normalized relative path", error))?;
        let target_path = data_root.join(relative_path);

        if file_type.is_dir() {
            ensure_target_directory(&target_path, local_applied)?;
            apply_directory_recursive(
                normalized_root,
                &source_path,
                data_root,
                copy_buffer,
                local_applied,
                is_cancelled,
            )?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|error| {
                    internal_error("Failed to create overlay parent directory", error)
                })?;
                local_applied.mark_target_changed();
            }
        }

        let mut reader = File::open(&source_path)
            .map_err(|error| internal_error("Failed to open normalized source file", error))?;
        let mut writer = create_target_file(&target_path, local_applied)?;
        let bytes_written = copy_stream_with_cancel(
            &mut reader,
            &mut writer,
            copy_buffer,
            is_cancelled,
            "Failed to read normalized source file",
            "Failed to write overlay output file",
        )?;
        local_applied.record_file_written(bytes_written);
    }

    Ok(())
}

fn ensure_target_directory(
    target_path: &Path,
    local_applied: &mut DataArchiveLocalMutationSummary,
) -> Result<(), DomainError> {
    if target_path.is_file() {
        fs::remove_file(target_path).map_err(|error| {
            internal_error(
                "Failed to replace file with directory while applying overlay",
                error,
            )
        })?;
        local_applied.mark_target_changed();
    }

    if !target_path.is_dir() {
        fs::create_dir_all(target_path)
            .map_err(|error| internal_error("Failed to create overlay directory", error))?;
        local_applied.mark_target_changed();
    }

    Ok(())
}

fn create_target_file(
    target_path: &Path,
    local_applied: &mut DataArchiveLocalMutationSummary,
) -> Result<File, DomainError> {
    if target_path.is_dir() {
        fs::remove_dir_all(target_path).map_err(|error| {
            internal_error(
                "Failed to replace directory with file while applying overlay",
                error,
            )
        })?;
        local_applied.mark_target_changed();
    }

    let file = File::create(target_path)
        .map_err(|error| internal_error("Failed to create overlay output file", error))?;
    local_applied.mark_target_changed();

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-data-archive-{}-{}",
            name,
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn apply_overlay_reports_local_mutation_when_cancelled_after_target_create() {
        let root = temp_root("apply-cancel");
        let normalized_root = root.join("normalized");
        let data_root = root.join("data");
        std::fs::create_dir_all(&normalized_root).expect("create normalized root");
        std::fs::create_dir_all(&data_root).expect("create data root");
        std::fs::write(normalized_root.join("settings.json"), b"{\"ok\":true}")
            .expect("write normalized file");

        let checks = AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, Ordering::SeqCst) >= 1;
        let mut progress = |_stage: &str, _percent: f32, _message: &str| {};

        let failure = apply_overlay(&normalized_root, &data_root, &mut progress, &is_cancelled)
            .expect_err("cancelled apply should fail");

        assert!(matches!(failure.error, DomainError::Cancelled(_)));
        assert!(failure.local_applied.changed());
        assert_eq!(failure.local_applied.files_written, 0);
        assert!(data_root.join("settings.json").exists());

        std::fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn apply_overlay_does_not_report_mutation_for_existing_directory_only_overlay() {
        let root = temp_root("apply-noop-dir");
        let normalized_root = root.join("normalized");
        let data_root = root.join("data");
        std::fs::create_dir_all(normalized_root.join("characters"))
            .expect("create normalized directory");
        std::fs::create_dir_all(data_root.join("characters")).expect("create target directory");

        let mut progress = |_stage: &str, _percent: f32, _message: &str| {};
        let summary = apply_overlay(&normalized_root, &data_root, &mut progress, &|| false)
            .expect("apply overlay");

        assert!(!summary.changed());

        std::fs::remove_dir_all(root).expect("remove temp root");
    }
}
