use std::fs;
use std::fs::File;
use std::path::Path;

use crate::domain::errors::DomainError;

use crate::infrastructure::persistence::data_archive::shared::{
    COPY_BUFFER_BYTES, copy_stream_with_cancel, create_output_file_replacing_directory,
    ensure_not_cancelled, ensure_output_directory, internal_error, read_directory_sorted,
};

pub fn apply_overlay(
    normalized_root: &Path,
    data_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    if !data_root.exists() {
        fs::create_dir_all(data_root).map_err(|error| {
            internal_error(
                "Failed to create data root directory before applying overlay",
                error,
            )
        })?;
    }

    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    apply_directory_recursive(
        normalized_root,
        normalized_root,
        data_root,
        &mut copy_buffer,
        is_cancelled,
    )?;

    report_progress("applying", 99.0, "Merge completed");
    Ok(())
}

fn apply_directory_recursive(
    normalized_root: &Path,
    current: &Path,
    data_root: &Path,
    copy_buffer: &mut [u8],
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
            ensure_output_directory(&target_path)?;
            apply_directory_recursive(
                normalized_root,
                &source_path,
                data_root,
                copy_buffer,
                is_cancelled,
            )?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                internal_error("Failed to create overlay parent directory", error)
            })?;
        }

        let mut reader = File::open(&source_path)
            .map_err(|error| internal_error("Failed to open normalized source file", error))?;
        let mut writer = create_output_file_replacing_directory(&target_path)?;
        copy_stream_with_cancel(
            &mut reader,
            &mut writer,
            copy_buffer,
            is_cancelled,
            "Failed to read normalized source file",
            "Failed to write overlay output file",
        )?;
    }

    Ok(())
}
