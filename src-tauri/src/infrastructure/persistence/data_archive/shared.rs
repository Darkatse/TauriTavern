use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path};

use crate::domain::errors::DomainError;

pub const DEFAULT_USER_HANDLE: &str = "default-user";

pub const USER_HANDLE_DIR_MARKERS: &[&str] = &[
    "characters",
    "chats",
    "User Avatars",
    "backgrounds",
    "thumbnails",
    "worlds",
    "user",
    "groups",
    "group chats",
    "backups",
    "NovelAI Settings",
    "KoboldAI Settings",
    "OpenAI Settings",
    "TextGen Settings",
    "themes",
    "movingUI",
    "extensions",
    "instruct",
    "context",
    "QuickReplies",
    "assets",
    "vectors",
    "sysprompt",
    "reasoning",
];

pub const USER_ROOT_DIR_MARKERS: &[&str] = &[
    "characters",
    "chats",
    "User Avatars",
    "backgrounds",
    "thumbnails",
    "worlds",
    "user",
    "groups",
    "group chats",
    "backups",
    "NovelAI Settings",
    "KoboldAI Settings",
    "OpenAI Settings",
    "TextGen Settings",
    "themes",
    "movingUI",
    "instruct",
    "context",
    "QuickReplies",
    "assets",
    "vectors",
    "sysprompt",
    "reasoning",
];

pub const USER_ROOT_FILE_MARKERS: &[&str] = &["settings.json"];

pub const MAX_ARCHIVE_ENTRIES: usize = 500_000;
pub const MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const MAX_ENTRY_UNCOMPRESSED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_COMPRESSION_RATIO: u64 = 500;
pub const COMPRESSION_RATIO_MIN_BYTES: u64 = 1024 * 1024;

pub const COPY_BUFFER_BYTES: usize = 4 * 1024 * 1024;
pub const FILE_IO_BUFFER_BYTES: usize = 4 * 1024 * 1024;
pub const PROGRESS_REPORT_MIN_DELTA: f32 = 0.5;

pub fn validate_zip_entry_limits(
    entry_name: &str,
    uncompressed_size: u64,
    compressed_size: u64,
    total_uncompressed_bytes: &mut u64,
) -> Result<(), DomainError> {
    if uncompressed_size > MAX_ENTRY_UNCOMPRESSED_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Archive entry is too large (>{} bytes): {}",
            MAX_ENTRY_UNCOMPRESSED_BYTES, entry_name
        )));
    }

    if compressed_size > 0
        && uncompressed_size > COMPRESSION_RATIO_MIN_BYTES
        && uncompressed_size / compressed_size > MAX_COMPRESSION_RATIO
    {
        return Err(DomainError::InvalidData(format!(
            "Archive entry compression ratio is suspicious: {}",
            entry_name
        )));
    }

    *total_uncompressed_bytes = total_uncompressed_bytes.saturating_add(uncompressed_size);
    if *total_uncompressed_bytes > MAX_TOTAL_UNCOMPRESSED_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Archive uncompressed size exceeds limit (>{} bytes)",
            MAX_TOTAL_UNCOMPRESSED_BYTES
        )));
    }

    Ok(())
}

pub fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

pub fn components_after_prefix(path: &Path, prefix: &Path) -> Option<Vec<String>> {
    let relative_path = if prefix.as_os_str().is_empty() {
        path
    } else {
        path.strip_prefix(prefix).ok()?
    };

    Some(path_components(relative_path))
}

pub fn is_user_root_marker(component: &str) -> bool {
    USER_ROOT_DIR_MARKERS.contains(&component) || USER_ROOT_FILE_MARKERS.contains(&component)
}

pub fn is_user_handle_marker(component: &str) -> bool {
    USER_HANDLE_DIR_MARKERS.contains(&component) || USER_ROOT_FILE_MARKERS.contains(&component)
}

pub fn normalize_zip_path(path: &Path) -> String {
    path_components(path).join("/")
}

pub fn read_directory_sorted(path: &Path) -> Result<Vec<fs::DirEntry>, DomainError> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| internal_error("Failed to read directory", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error("Failed to read directory entry", error))?;

    entries.sort_by(|left, right| {
        left.file_name()
            .to_string_lossy()
            .cmp(&right.file_name().to_string_lossy())
    });

    Ok(entries)
}

pub fn copy_stream_with_cancel<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    copy_buffer: &mut [u8],
    is_cancelled: &dyn Fn() -> bool,
    read_error_context: &str,
    write_error_context: &str,
) -> Result<(), DomainError> {
    loop {
        ensure_not_cancelled(is_cancelled)?;

        let bytes_read = reader
            .read(copy_buffer)
            .map_err(|error| internal_error(read_error_context, error))?;
        if bytes_read == 0 {
            break;
        }

        writer
            .write_all(&copy_buffer[..bytes_read])
            .map_err(|error| internal_error(write_error_context, error))?;
    }

    Ok(())
}

pub fn ensure_output_directory(path: &Path) -> Result<(), DomainError> {
    if path.is_file() {
        fs::remove_file(path).map_err(|error| {
            internal_error(
                "Failed to replace file with directory in normalized output",
                error,
            )
        })?;
    }

    fs::create_dir_all(path).map_err(|error| internal_error("Failed to create directory", error))
}

pub fn create_output_file_replacing_directory(path: &Path) -> Result<File, DomainError> {
    match File::create(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::IsADirectory => {
            fs::remove_dir_all(path).map_err(|remove_error| {
                internal_error(
                    "Failed to replace directory with file while applying overlay",
                    remove_error,
                )
            })?;

            File::create(path).map_err(|create_error| {
                internal_error("Failed to create overlay output file", create_error)
            })
        }
        Err(error) => Err(internal_error(
            "Failed to create overlay output file",
            error,
        )),
    }
}

pub fn cleanup_directory_sync(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!("Failed to clean up directory {}: {}", path.display(), error);
        }
    }
}

pub fn ensure_not_cancelled(is_cancelled: &dyn Fn() -> bool) -> Result<(), DomainError> {
    if is_cancelled() {
        return Err(DomainError::InternalError(
            crate::infrastructure::persistence::data_archive::CANCELLED_ERROR_MARKER.to_string(),
        ));
    }

    Ok(())
}

pub fn progress_percent(processed: u64, total: u64, min: f32, max: f32) -> f32 {
    if total == 0 {
        return max;
    }

    let ratio = (processed as f64 / total as f64).clamp(0.0, 1.0) as f32;
    min + (max - min) * ratio
}

pub fn internal_error(context: &str, error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("{}: {}", context, error))
}

pub fn collect_user_handles_from_components(
    components: &[String],
    user_handles: &mut BTreeSet<String>,
) {
    if components.len() < 2 {
        return;
    }

    let handle = components[0].clone();
    let second = components[1].as_str();
    if is_user_handle_marker(second) {
        user_handles.insert(handle);
    }
}
