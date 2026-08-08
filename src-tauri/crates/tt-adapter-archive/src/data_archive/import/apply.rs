use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;
use tt_domain::models::data_archive::{DataArchiveImportFailure, DataArchiveLocalMutationSummary};

use crate::data_archive::shared::{
    ByteProgress, COPY_BUFFER_BYTES, copy_stream_with_cancel, ensure_not_cancelled, internal_error,
    read_directory_sorted,
};

pub fn apply_overlay(
    normalized_root: &Path,
    data_root: &Path,
    total_bytes: u64,
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
    let mut progress = ByteProgress::new(total_bytes, 92.0, 99.0);
    if let Err(error) = apply_directory_recursive(
        normalized_root,
        data_root,
        &mut copy_buffer,
        &mut progress,
        report_progress,
        &mut local_applied,
        is_cancelled,
    ) {
        return Err(DataArchiveImportFailure::new(error, local_applied));
    }

    progress.complete("applying", "Merge completed", report_progress);
    Ok(local_applied)
}

fn apply_directory_recursive(
    source_directory: &Path,
    target_directory: &Path,
    copy_buffer: &mut [u8],
    progress: &mut ByteProgress,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    local_applied: &mut DataArchiveLocalMutationSummary,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    for entry in read_directory_sorted(source_directory)? {
        ensure_not_cancelled(is_cancelled)?;

        let source_path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read normalized entry type", error))?;
        let target_path = target_directory.join(entry.file_name());

        if file_type.is_dir() {
            ensure_target_directory(&target_path, local_applied)?;
            apply_directory_recursive(
                &source_path,
                &target_path,
                copy_buffer,
                progress,
                report_progress,
                local_applied,
                is_cancelled,
            )?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        if let Some(parent) = target_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent).map_err(|error| {
                internal_error("Failed to create overlay parent directory", error)
            })?;
            local_applied.mark_target_changed();
        }

        move_or_copy_file_into_place(
            &source_path,
            &target_path,
            copy_buffer,
            progress,
            report_progress,
            local_applied,
            is_cancelled,
        )?;
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

fn move_or_copy_file_into_place(
    source_path: &Path,
    target_path: &Path,
    copy_buffer: &mut [u8],
    progress: &mut ByteProgress,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    local_applied: &mut DataArchiveLocalMutationSummary,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    let source_size = source_path
        .metadata()
        .map_err(|error| internal_error("Failed to read normalized source metadata", error))?
        .len();
    let temp_path = overlay_temp_path(target_path);
    let copied = match fs::rename(source_path, &temp_path) {
        Ok(()) => false,
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_file_to_temp(
                source_path,
                &temp_path,
                copy_buffer,
                progress,
                report_progress,
                is_cancelled,
            )?;
            true
        }
        Err(error) => {
            return Err(internal_error(
                "Failed to move normalized source file into place",
                error,
            ));
        }
    };

    if let Err(error) = ensure_not_cancelled(is_cancelled) {
        cleanup_temp_file(&temp_path);
        return Err(error);
    }

    replace_temp_file(&temp_path, target_path, local_applied)?;
    local_applied.record_file_written(source_size);
    if !copied {
        progress.advance(
            source_size,
            "applying",
            "Merging data directory",
            report_progress,
        );
    }

    Ok(())
}

fn copy_file_to_temp(
    source_path: &Path,
    temp_path: &Path,
    copy_buffer: &mut [u8],
    progress: &mut ByteProgress,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    let mut reader = File::open(source_path)
        .map_err(|error| internal_error("Failed to open normalized source file", error))?;
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|error| internal_error("Failed to create overlay temp file", error))?;

    let mut on_bytes_copied = |bytes| {
        progress.advance(bytes, "applying", "Merging data directory", report_progress);
    };
    if let Err(error) = copy_stream_with_cancel(
        &mut reader,
        &mut writer,
        copy_buffer,
        is_cancelled,
        &mut on_bytes_copied,
        "Failed to read normalized source file",
        "Failed to write overlay temp file",
    ) {
        drop(writer);
        cleanup_temp_file(temp_path);
        return Err(error);
    }

    if let Err(error) = writer.flush() {
        drop(writer);
        cleanup_temp_file(temp_path);
        return Err(internal_error("Failed to flush overlay temp file", error));
    }

    Ok(())
}

fn overlay_temp_path(target_path: &Path) -> PathBuf {
    target_path.with_file_name(format!(
        ".tauritavern-import-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ))
}

fn replace_temp_file(
    temp_path: &Path,
    target_path: &Path,
    local_applied: &mut DataArchiveLocalMutationSummary,
) -> Result<(), DomainError> {
    let target_existed = target_path.exists();

    if target_path.is_dir() {
        if let Err(error) = fs::remove_dir_all(target_path) {
            cleanup_temp_file(temp_path);
            return Err(internal_error(
                "Failed to replace directory with file while applying overlay",
                error,
            ));
        }
        local_applied.mark_target_changed();
    }

    match fs::rename(temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(error) => {
            if target_existed && !target_path.exists() {
                local_applied.mark_target_changed();
            }
            cleanup_temp_file(temp_path);
            Err(internal_error(
                "Failed to replace overlay output file",
                error,
            ))
        }
    }
}

fn cleanup_temp_file(temp_path: &Path) {
    if let Err(error) = fs::remove_file(temp_path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(
            "Failed to clean up overlay temp file {}: {}",
            temp_path.display(),
            error
        );
    }
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

    fn assert_no_overlay_temp_files(dir: &std::path::Path) {
        let has_temp = std::fs::read_dir(dir)
            .expect("read temp parent")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tauritavern-import-")
            });
        assert!(!has_temp, "overlay temp file should be cleaned up");
    }

    #[test]
    fn apply_overlay_preserves_existing_file_when_cancelled_after_same_volume_move() {
        let root = temp_root("apply-cancel-after-copy");
        let normalized_root = root.join("normalized");
        let data_root = root.join("data");
        std::fs::create_dir_all(&normalized_root).expect("create normalized root");
        std::fs::create_dir_all(&data_root).expect("create data root");
        std::fs::write(normalized_root.join("settings.json"), b"new")
            .expect("write normalized file");
        std::fs::write(data_root.join("settings.json"), b"old").expect("write target file");

        let checks = AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, Ordering::SeqCst) >= 1;
        let mut progress = |_stage: &str, _percent: f32, _message: &str| {};

        let failure = apply_overlay(
            &normalized_root,
            &data_root,
            3,
            &mut progress,
            &is_cancelled,
        )
        .expect_err("cancelled apply should fail");

        assert!(matches!(failure.error, DomainError::Cancelled(_)));
        assert!(!failure.local_applied.changed());
        assert_eq!(
            std::fs::read(data_root.join("settings.json")).expect("read target file"),
            b"old"
        );
        assert_no_overlay_temp_files(&data_root);

        std::fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn apply_overlay_preserves_directory_when_cancelled_after_same_volume_move() {
        let root = temp_root("apply-cancel-dir");
        let normalized_root = root.join("normalized");
        let data_root = root.join("data");
        let target_path = data_root.join("settings.json");
        std::fs::create_dir_all(&normalized_root).expect("create normalized root");
        std::fs::create_dir_all(target_path.join("child")).expect("create target directory");
        std::fs::write(normalized_root.join("settings.json"), b"new")
            .expect("write normalized file");
        std::fs::write(target_path.join("child/old.txt"), b"old").expect("write target child");

        let checks = AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, Ordering::SeqCst) >= 1;
        let mut progress = |_stage: &str, _percent: f32, _message: &str| {};

        let failure = apply_overlay(
            &normalized_root,
            &data_root,
            3,
            &mut progress,
            &is_cancelled,
        )
        .expect_err("cancelled apply should fail");

        assert!(matches!(failure.error, DomainError::Cancelled(_)));
        assert!(!failure.local_applied.changed());
        assert!(target_path.is_dir());
        assert_eq!(
            std::fs::read(target_path.join("child/old.txt")).expect("read target child"),
            b"old"
        );
        assert_no_overlay_temp_files(&data_root);

        std::fs::remove_dir_all(root).expect("remove temp root");
    }

    #[test]
    fn apply_overlay_replaces_directory_with_file_by_same_volume_move() {
        let root = temp_root("apply-dir-to-file");
        let normalized_root = root.join("normalized");
        let data_root = root.join("data");
        let target_path = data_root.join("settings.json");
        std::fs::create_dir_all(&normalized_root).expect("create normalized root");
        std::fs::create_dir_all(target_path.join("child")).expect("create target directory");
        std::fs::write(normalized_root.join("settings.json"), b"new")
            .expect("write normalized file");
        std::fs::write(target_path.join("child/old.txt"), b"old").expect("write target child");

        let mut reports = Vec::new();
        let mut report_progress = |stage: &str, percent: f32, _message: &str| {
            if stage == "applying" {
                reports.push(percent);
            }
        };
        let summary = apply_overlay(
            &normalized_root,
            &data_root,
            3,
            &mut report_progress,
            &|| false,
        )
        .expect("apply overlay");

        assert!(summary.changed());
        assert_eq!(summary.files_written, 1);
        assert_eq!(summary.bytes_written, 3);
        assert_eq!(
            std::fs::read(&target_path).expect("read target file"),
            b"new"
        );
        assert!(!normalized_root.join("settings.json").exists());
        assert_eq!(reports.last().copied(), Some(99.0));

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
        let summary = apply_overlay(&normalized_root, &data_root, 0, &mut progress, &|| false)
            .expect("apply overlay");

        assert!(!summary.changed());

        std::fs::remove_dir_all(root).expect("remove temp root");
    }
}
