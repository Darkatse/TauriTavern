use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;
use tt_domain::models::data_archive::{DataArchiveImportFailure, DataArchiveLocalMutationSummary};

use crate::data_archive::shared::{
    ByteProgress, COPY_BUFFER_BYTES, cleanup_directory_sync, copy_stream_with_cancel,
    ensure_not_cancelled, internal_error, read_directory_sorted,
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
        &data_root.join("_tauritavern/databases"),
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

#[allow(clippy::too_many_arguments)]
fn apply_directory_recursive(
    source_directory: &Path,
    target_directory: &Path,
    database_root: &Path,
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
            if target_directory == database_root {
                replace_database_directory(
                    &source_path,
                    &target_path,
                    database_root,
                    copy_buffer,
                    progress,
                    report_progress,
                    local_applied,
                    is_cancelled,
                )?;
                continue;
            }
            ensure_target_directory(&target_path, local_applied)?;
            apply_directory_recursive(
                &source_path,
                &target_path,
                database_root,
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

/// A database and its sidecars are one restore unit. Stage beside the destination
/// so cancellation/copy errors leave the previous database intact, including when
/// the archive workspace and data root are on different filesystems.
#[allow(clippy::too_many_arguments)]
fn replace_database_directory(
    source_path: &Path,
    target_path: &Path,
    database_root: &Path,
    copy_buffer: &mut [u8],
    progress: &mut ByteProgress,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    local_applied: &mut DataArchiveLocalMutationSummary,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    let staged_path = overlay_temp_path(target_path);
    fs::create_dir(&staged_path)
        .map_err(|error| internal_error("Failed to stage database import", error))?;
    let mut staged = DataArchiveLocalMutationSummary::default();
    let result = apply_directory_recursive(
        source_path,
        &staged_path,
        database_root,
        copy_buffer,
        progress,
        report_progress,
        &mut staged,
        is_cancelled,
    )
    .and_then(|()| ensure_not_cancelled(is_cancelled))
    .and_then(|()| replace_directory(&staged_path, target_path, local_applied));
    if result.is_err() {
        cleanup_directory_sync(&staged_path);
    }
    result?;
    local_applied.files_written = local_applied
        .files_written
        .saturating_add(staged.files_written);
    local_applied.bytes_written = local_applied
        .bytes_written
        .saturating_add(staged.bytes_written);
    local_applied.mark_target_changed();
    Ok(())
}

fn replace_directory(
    staged_path: &Path,
    target_path: &Path,
    local_applied: &mut DataArchiveLocalMutationSummary,
) -> Result<(), DomainError> {
    let previous_path = overlay_temp_path(target_path);
    let had_previous = target_path.exists();
    if had_previous {
        fs::rename(target_path, &previous_path)
            .map_err(|error| internal_error("Failed to move previous database aside", error))?;
    }
    if let Err(error) = fs::rename(staged_path, target_path) {
        if had_previous && let Err(restore_error) = fs::rename(&previous_path, target_path) {
            local_applied.mark_target_changed();
            return Err(DomainError::InternalError(format!(
                "Database import failed: {error}; restoring the previous database failed: \
                 {restore_error}; previous data remains at {}",
                previous_path.display()
            )));
        }
        return Err(internal_error("Failed to publish imported database", error));
    }
    if had_previous {
        if previous_path.is_dir() {
            cleanup_directory_sync(&previous_path);
        } else {
            cleanup_temp_file(&previous_path);
        }
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

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
    fn database_restore_replaces_the_file_group_without_touching_other_namespaces() {
        let root = temp_root("database-restore");
        let normalized_root = root.join("normalized");
        let data_root = root.join("data");
        let relative = "_tauritavern/databases/db-memory";
        let source = normalized_root.join(relative);
        let target = data_root.join(relative);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        for name in ["database.tdb", "database.tdb.vec"] {
            fs::write(source.join(name), b"new").unwrap();
            fs::write(target.join(name), b"old").unwrap();
        }
        fs::write(target.join("database.tdb.text"), b"stale index").unwrap();
        let other = data_root.join("_tauritavern/databases/db-other");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("database.tdb"), b"keep").unwrap();

        let summary = apply_overlay(&normalized_root, &data_root, 6, &mut |_, _, _| {}, &|| {
            false
        })
        .unwrap();

        assert_eq!(summary.files_written, 2);
        assert_eq!(summary.bytes_written, 6);
        for name in ["database.tdb", "database.tdb.vec"] {
            assert_eq!(fs::read(target.join(name)).unwrap(), b"new");
        }
        assert!(!target.join("database.tdb.text").exists());
        assert_eq!(fs::read(other.join("database.tdb")).unwrap(), b"keep");
        assert_no_overlay_temp_files(target.parent().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelling_database_staging_keeps_the_previous_file_group_intact() {
        let root = temp_root("database-cancel");
        let normalized_root = root.join("normalized");
        let data_root = root.join("data");
        let relative = "_tauritavern/databases/db-memory";
        let source = normalized_root.join(relative);
        let target = data_root.join(relative);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        for name in ["database.tdb", "database.tdb.vec"] {
            fs::write(source.join(name), b"new").unwrap();
            fs::write(target.join(name), b"old").unwrap();
        }
        fs::write(target.join("database.tdb.text"), b"keep index").unwrap();
        let cancelled = AtomicBool::new(false);
        let failure = apply_overlay(
            &normalized_root,
            &data_root,
            6,
            &mut |_, _, _| cancelled.store(true, Ordering::SeqCst),
            &|| cancelled.load(Ordering::SeqCst),
        )
        .unwrap_err();

        assert!(matches!(failure.error, DomainError::Cancelled(_)));
        assert!(!failure.local_applied.changed());
        for name in ["database.tdb", "database.tdb.vec"] {
            assert_eq!(fs::read(target.join(name)).unwrap(), b"old");
        }
        assert_eq!(
            fs::read(target.join("database.tdb.text")).unwrap(),
            b"keep index"
        );
        assert_no_overlay_temp_files(target.parent().unwrap());
        fs::remove_dir_all(root).unwrap();
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
