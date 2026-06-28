use chrono::Utc;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Seek, Write};
use std::path::Path;
use zip::write::SimpleFileOptions as FileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::domain::errors::DomainError;
use crate::infrastructure::zipkit::export_file_options;

use super::DataArchiveExportResult;
use super::shared::{
    COPY_BUFFER_BYTES, FILE_IO_BUFFER_BYTES, PROGRESS_REPORT_MIN_DELTA, copy_stream_with_cancel,
    ensure_not_cancelled, internal_error, normalize_archive_entry_path, path_components,
    progress_percent, read_directory_sorted,
};

#[derive(Debug, Clone)]
struct ExportProgress {
    processed_steps: u64,
    total_steps: u64,
    last_reported_percent: f32,
}

pub fn run_export_data_archive(
    data_root: &Path,
    output_path: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DataArchiveExportResult, DomainError> {
    run_export_archive(
        data_root,
        output_path,
        "data",
        &|_| true,
        report_progress,
        is_cancelled,
    )
}

pub fn run_export_user_backup_archive(
    user_root: &Path,
    output_path: &Path,
    include_secrets: bool,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DataArchiveExportResult, DomainError> {
    run_export_archive(
        user_root,
        output_path,
        "",
        &|relative_path| should_include_user_backup_entry(relative_path, include_secrets),
        report_progress,
        is_cancelled,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_export_archive(
    source_root: &Path,
    output_path: &Path,
    archive_root_prefix: &str,
    include_entry: &dyn Fn(&Path) -> bool,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DataArchiveExportResult, DomainError> {
    report_progress("preparing", 0.0, "Preparing export");
    ensure_not_cancelled(is_cancelled)?;

    if !source_root.is_dir() {
        return Err(DomainError::NotFound(format!(
            "Export source directory not found: {}",
            source_root.display()
        )));
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| internal_error("Failed to create export output directory", error))?;
    }

    let normalized_archive_root_prefix = archive_root_prefix.trim_matches('/');
    let root_step_count = u64::from(!normalized_archive_root_prefix.is_empty());
    let total_steps = count_export_entries(source_root, source_root, include_entry, is_cancelled)?
        .saturating_add(root_step_count);
    let mut progress = ExportProgress {
        processed_steps: 0,
        total_steps,
        last_reported_percent: 0.0,
    };

    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);

    let output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .map_err(|error| internal_error("Failed to create export archive file", error))?;
    let buffered_output = BufWriter::with_capacity(FILE_IO_BUFFER_BYTES, output_file);
    let mut writer = ZipWriter::new(buffered_output);

    if !normalized_archive_root_prefix.is_empty() {
        writer
            .add_directory(format!("{}/", normalized_archive_root_prefix), dir_options)
            .map_err(|error| internal_error("Failed to add archive root directory", error))?;
        progress.processed_steps = progress.processed_steps.saturating_add(1);
        report_export_progress(&mut progress, report_progress);
    }

    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    write_export_entries(
        &mut writer,
        source_root,
        source_root,
        normalized_archive_root_prefix,
        include_entry,
        dir_options,
        &mut progress,
        &mut copy_buffer,
        report_progress,
        is_cancelled,
    )?;

    let mut buffered_output = writer
        .finish()
        .map_err(|error| internal_error("Failed to finalize export archive", error))?;
    buffered_output
        .flush()
        .map_err(|error| internal_error("Failed to flush export archive", error))?;

    ensure_not_cancelled(is_cancelled)?;
    report_progress("finalizing", 100.0, "Export completed");

    Ok(DataArchiveExportResult {
        archive_path: output_path.to_path_buf(),
    })
}

pub fn default_export_file_name() -> String {
    format!(
        "tauritavern-data-{}.zip",
        Utc::now().format("%Y%m%d-%H%M%S")
    )
}

fn count_export_entries(
    root: &Path,
    current: &Path,
    include_entry: &dyn Fn(&Path) -> bool,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<u64, DomainError> {
    let mut count = 0u64;

    for entry in read_directory_sorted(current)? {
        ensure_not_cancelled(is_cancelled)?;

        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read export entry type", error))?;
        let path = entry.path();
        let relative_path = path
            .strip_prefix(root)
            .map_err(|error| internal_error("Failed to resolve export relative path", error))?;
        if !include_entry(relative_path) {
            continue;
        }

        if file_type.is_dir() {
            count = count.saturating_add(1);
            count = count.saturating_add(count_export_entries(
                root,
                &path,
                include_entry,
                is_cancelled,
            )?);
            continue;
        }

        if file_type.is_file() {
            count = count.saturating_add(1);
        }
    }

    Ok(count)
}

#[allow(clippy::too_many_arguments)]
fn write_export_entries(
    writer: &mut ZipWriter<impl Write + Seek>,
    root: &Path,
    current: &Path,
    archive_root_prefix: &str,
    include_entry: &dyn Fn(&Path) -> bool,
    dir_options: FileOptions,
    progress: &mut ExportProgress,
    copy_buffer: &mut [u8],
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    for entry in read_directory_sorted(current)? {
        ensure_not_cancelled(is_cancelled)?;

        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read export entry type", error))?;
        let relative_path = path
            .strip_prefix(root)
            .map_err(|error| internal_error("Failed to resolve export relative path", error))?;
        if !include_entry(relative_path) {
            continue;
        }

        let archive_relative_path = normalize_archive_entry_path(relative_path);
        let entry_path = archive_entry_path(archive_root_prefix, &archive_relative_path);

        if file_type.is_dir() {
            writer
                .add_directory(format!("{}/", entry_path), dir_options)
                .map_err(|error| internal_error("Failed to add directory to archive", error))?;
            progress.processed_steps = progress.processed_steps.saturating_add(1);
            report_export_progress(progress, report_progress);

            write_export_entries(
                writer,
                root,
                &path,
                archive_root_prefix,
                include_entry,
                dir_options,
                progress,
                copy_buffer,
                report_progress,
                is_cancelled,
            )?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let file_options = export_file_options(&path);
        writer
            .start_file(&entry_path, file_options)
            .map_err(|error| internal_error("Failed to add file to archive", error))?;

        let mut source_file = File::open(&path)
            .map_err(|error| internal_error("Failed to open export source file", error))?;
        copy_stream_with_cancel(
            &mut source_file,
            writer,
            copy_buffer,
            is_cancelled,
            "Failed to read export source file",
            "Failed to write file to archive",
        )?;

        progress.processed_steps = progress.processed_steps.saturating_add(1);
        report_export_progress(progress, report_progress);
    }

    Ok(())
}

fn archive_entry_path(archive_root_prefix: &str, archive_relative_path: &str) -> String {
    if archive_root_prefix.is_empty() {
        return archive_relative_path.to_string();
    }

    format!("{}/{}", archive_root_prefix, archive_relative_path)
}

fn should_include_user_backup_entry(relative_path: &Path, include_secrets: bool) -> bool {
    if include_secrets {
        return true;
    }

    let components = path_components(relative_path);
    match components.as_slice() {
        [file_name] => file_name != "secrets.json",
        [directory, file_name] if directory == "backups" => {
            !(file_name.starts_with("secrets_migration_") && file_name.ends_with(".json"))
        }
        _ => true,
    }
}

fn report_export_progress(
    progress: &mut ExportProgress,
    report_progress: &mut dyn FnMut(&str, f32, &str),
) {
    let percent = progress_percent(progress.processed_steps, progress.total_steps, 3.0, 96.0);
    let should_report = progress.processed_steps >= progress.total_steps
        || percent - progress.last_reported_percent >= PROGRESS_REPORT_MIN_DELTA;
    if !should_report {
        return;
    }

    progress.last_reported_percent = percent;
    report_progress("zipping", percent, "Writing archive entries");
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-data-archive-export-{}-{}",
            name,
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn user_backup_filter_excludes_secret_files_when_secret_export_is_disabled() {
        assert!(!should_include_user_backup_entry(
            Path::new("secrets.json"),
            false
        ));
        assert!(!should_include_user_backup_entry(
            Path::new("backups/secrets_migration_123.json"),
            false
        ));
    }

    #[test]
    fn user_backup_filter_keeps_regular_files_when_secret_export_is_disabled() {
        assert!(should_include_user_backup_entry(
            Path::new("settings.json"),
            false
        ));
        assert!(should_include_user_backup_entry(
            Path::new("backups/chat.jsonl"),
            false
        ));
        assert!(should_include_user_backup_entry(
            Path::new("characters/secrets.json"),
            false
        ));
    }

    #[test]
    fn user_backup_filter_keeps_secret_files_when_secret_export_is_enabled() {
        assert!(should_include_user_backup_entry(
            Path::new("secrets.json"),
            true
        ));
        assert!(should_include_user_backup_entry(
            Path::new("backups/secrets_migration_123.json"),
            true
        ));
    }

    #[test]
    fn export_refuses_to_overwrite_existing_archive() {
        let root = temp_root("existing-output");
        let source_root = root.join("source");
        let output_path = root.join("export.zip");
        fs::create_dir_all(&source_root).expect("create source root");
        fs::write(source_root.join("settings.json"), b"{}").expect("write source file");
        fs::write(&output_path, b"keep me").expect("write existing output");

        let mut report_progress = |_stage: &str, _progress_percent: f32, _message: &str| {};
        let result =
            run_export_data_archive(&source_root, &output_path, &mut report_progress, &|| false);

        assert!(result.is_err());
        assert_eq!(
            fs::read(&output_path).expect("read existing output"),
            b"keep me"
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }
}
