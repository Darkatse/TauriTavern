use chrono::Utc;
use std::fs::{self, File};
use std::io::{BufWriter, Seek, Write};
use std::path::Path;
use zip::write::SimpleFileOptions as FileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::domain::errors::DomainError;
use crate::infrastructure::zipkit::export_file_options;

use super::DataArchiveExportResult;
use super::shared::{
    COPY_BUFFER_BYTES, FILE_IO_BUFFER_BYTES, PROGRESS_REPORT_MIN_DELTA, copy_stream_with_cancel,
    ensure_not_cancelled, internal_error, normalize_zip_path, progress_percent,
    read_directory_sorted,
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
    report_progress("preparing", 0.0, "Preparing export");
    ensure_not_cancelled(is_cancelled)?;

    if !data_root.is_dir() {
        return Err(DomainError::NotFound(format!(
            "Data directory not found: {}",
            data_root.display()
        )));
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| internal_error("Failed to create export output directory", error))?;
    }

    let total_steps = count_export_entries(data_root, is_cancelled)?.saturating_add(1);
    let mut progress = ExportProgress {
        processed_steps: 1,
        total_steps,
        last_reported_percent: 0.0,
    };

    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);

    let output_file = File::create(output_path)
        .map_err(|error| internal_error("Failed to create export archive file", error))?;
    let buffered_output = BufWriter::with_capacity(FILE_IO_BUFFER_BYTES, output_file);
    let mut writer = ZipWriter::new(buffered_output);

    writer
        .add_directory("data/", dir_options)
        .map_err(|error| internal_error("Failed to add archive root directory", error))?;
    report_export_progress(&mut progress, report_progress);

    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    write_export_entries(
        &mut writer,
        data_root,
        data_root,
        "data",
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
        file_name: output_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("tauritavern-data.zip")
            .to_string(),
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
    current: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<u64, DomainError> {
    let mut count = 0u64;

    for entry in read_directory_sorted(current)? {
        ensure_not_cancelled(is_cancelled)?;

        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read export entry type", error))?;
        let path = entry.path();

        if file_type.is_dir() {
            count = count.saturating_add(1);
            count = count.saturating_add(count_export_entries(&path, is_cancelled)?);
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
    zip_prefix: &str,
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
        let zip_relative = normalize_zip_path(relative_path);
        let zip_path = format!("{}/{}", zip_prefix, zip_relative);

        if file_type.is_dir() {
            writer
                .add_directory(format!("{}/", zip_path), dir_options)
                .map_err(|error| internal_error("Failed to add directory to archive", error))?;
            progress.processed_steps = progress.processed_steps.saturating_add(1);
            report_export_progress(progress, report_progress);

            write_export_entries(
                writer,
                root,
                &path,
                zip_prefix,
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
            .start_file(&zip_path, file_options)
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
