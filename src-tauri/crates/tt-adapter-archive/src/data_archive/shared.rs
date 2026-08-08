use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path};

use tt_domain::errors::DomainError;

pub const IMPORT_TARGET_USER_HANDLE: &str = "default-user";
pub const MACOS_RESOURCE_FORK_ROOT: &str = "__MACOSX";

// Root-level extensions/third-party is a full data-root signal, not a user-backup marker.
pub const USER_HANDLE_ROOT_CHILD_DIR_MARKERS: &[&str] = &[
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

pub const SILLYTAVERN_USER_ROOT_DIR_MARKERS: &[&str] = &[
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

pub const SILLYTAVERN_USER_ROOT_FILE_MARKERS: &[&str] = &["settings.json"];

pub const MAX_ARCHIVE_ENTRIES: usize = 500_000;
pub const MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const MAX_ENTRY_UNCOMPRESSED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_COMPRESSION_RATIO: u64 = 500;
pub const COMPRESSION_RATIO_MIN_BYTES: u64 = 1024 * 1024;

pub const COPY_BUFFER_BYTES: usize = 4 * 1024 * 1024;
pub const FILE_IO_BUFFER_BYTES: usize = 4 * 1024 * 1024;
pub const PROGRESS_REPORT_MIN_DELTA: f32 = 0.5;

#[derive(Debug)]
pub struct ByteProgress {
    processed_bytes: u64,
    total_bytes: u64,
    min_percent: f32,
    max_percent: f32,
    last_reported_percent: f32,
}

impl ByteProgress {
    pub fn new(total_bytes: u64, min_percent: f32, max_percent: f32) -> Self {
        Self {
            processed_bytes: 0,
            total_bytes,
            min_percent,
            max_percent,
            last_reported_percent: min_percent,
        }
    }

    pub fn advance(
        &mut self,
        bytes: u64,
        stage: &str,
        message: &str,
        report_progress: &mut dyn FnMut(&str, f32, &str),
    ) {
        self.processed_bytes = self.processed_bytes.saturating_add(bytes);
        let percent = progress_percent(
            self.processed_bytes,
            self.total_bytes,
            self.min_percent,
            self.max_percent,
        );
        let delta = percent - self.last_reported_percent;
        if percent <= self.last_reported_percent
            || (self.processed_bytes < self.total_bytes && delta < PROGRESS_REPORT_MIN_DELTA)
        {
            return;
        }

        self.last_reported_percent = percent;
        report_progress(stage, percent, message);
    }

    pub fn complete(
        &mut self,
        stage: &str,
        message: &str,
        report_progress: &mut dyn FnMut(&str, f32, &str),
    ) {
        self.last_reported_percent = self.max_percent;
        report_progress(stage, self.max_percent, message);
    }
}

pub fn validate_archive_entry_limits(
    entry_name: &str,
    uncompressed_size: u64,
    compressed_size: Option<u64>,
    total_uncompressed_bytes: &mut u64,
) -> Result<(), DomainError> {
    if uncompressed_size > MAX_ENTRY_UNCOMPRESSED_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Archive entry is too large (>{} bytes): {}",
            MAX_ENTRY_UNCOMPRESSED_BYTES, entry_name
        )));
    }

    if let Some(compressed_size) = compressed_size
        && compressed_size > 0
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

pub fn validate_archive_compression_ratio(
    format_name: &str,
    uncompressed_size: u64,
    compressed_size: Option<u64>,
) -> Result<(), DomainError> {
    let Some(compressed_size) = compressed_size else {
        return Ok(());
    };

    if compressed_size > 0
        && uncompressed_size > COMPRESSION_RATIO_MIN_BYTES
        && uncompressed_size / compressed_size > MAX_COMPRESSION_RATIO
    {
        return Err(DomainError::InvalidData(format!(
            "Archive compression ratio is suspicious: {}",
            format_name
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

pub fn is_sillytavern_user_root_entry(component: &str) -> bool {
    SILLYTAVERN_USER_ROOT_DIR_MARKERS.contains(&component)
        || SILLYTAVERN_USER_ROOT_FILE_MARKERS.contains(&component)
}

pub fn is_user_handle_root_child_entry(component: &str) -> bool {
    USER_HANDLE_ROOT_CHILD_DIR_MARKERS.contains(&component)
        || SILLYTAVERN_USER_ROOT_FILE_MARKERS.contains(&component)
}

pub fn is_macos_resource_fork_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Normal(component)) if component == OsStr::new(MACOS_RESOURCE_FORK_ROOT)
    )
}

pub fn normalize_archive_entry_path(path: &Path) -> String {
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

pub fn copy_stream_with_cancel<R: Read + ?Sized, W: Write + ?Sized>(
    reader: &mut R,
    writer: &mut W,
    copy_buffer: &mut [u8],
    is_cancelled: &dyn Fn() -> bool,
    on_bytes_copied: &mut dyn FnMut(u64),
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
        on_bytes_copied(bytes_read as u64);
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
    if let Err(error) = fs::remove_dir_all(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!("Failed to clean up directory {}: {}", path.display(), error);
    }
}

pub fn ensure_not_cancelled(is_cancelled: &dyn Fn() -> bool) -> Result<(), DomainError> {
    if is_cancelled() {
        return Err(DomainError::cancelled("Job cancelled"));
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn stream_copy_reports_bytes_and_stops_at_the_next_cancel_check() {
        let mut reader = Cursor::new(b"abcdefgh".to_vec());
        let mut writer = Vec::new();
        let mut buffer = [0u8; 4];
        let checks = AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, Ordering::SeqCst) >= 1;
        let mut copied_chunks = Vec::new();
        let mut on_bytes_copied = |bytes| copied_chunks.push(bytes);

        let error = copy_stream_with_cancel(
            &mut reader,
            &mut writer,
            &mut buffer,
            &is_cancelled,
            &mut on_bytes_copied,
            "Failed to read test stream",
            "Failed to write test stream",
        )
        .expect_err("second chunk should be cancelled");

        assert!(matches!(error, DomainError::Cancelled(_)));
        assert_eq!(writer, b"abcd");
        assert_eq!(copied_chunks, vec![4]);
    }

    #[test]
    fn byte_progress_complete_reports_the_terminal_message() {
        let mut progress = ByteProgress::new(4, 10.0, 20.0);
        let mut reports = Vec::new();
        let mut report = |stage: &str, percent: f32, message: &str| {
            reports.push((stage.to_string(), percent, message.to_string()));
        };

        progress.advance(4, "copying", "Copying", &mut report);
        progress.complete("copying", "Copied", &mut report);

        assert_eq!(
            reports,
            vec![
                ("copying".to_string(), 20.0, "Copying".to_string()),
                ("copying".to_string(), 20.0, "Copied".to_string()),
            ]
        );
    }
}
