use std::path::Path;
use std::str;

use serde_json::Value;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::file_system::replace_file_with_fallback;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_repository::ChatPayloadCursor;

pub(super) const WINDOW_READ_CHUNK_BYTES: usize = 64 * 1024;

pub(super) fn payload_not_found(path: &Path) -> DomainError {
    DomainError::NotFound(format!("Chat payload not found: {:?}", path))
}

pub(super) fn map_open_existing_error(path: &Path, error: std::io::Error) -> DomainError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return payload_not_found(path);
    }

    DomainError::InternalError(format!(
        "Failed to open chat payload file {:?}: {}",
        path, error
    ))
}

pub(super) fn map_existing_metadata_error(path: &Path, error: std::io::Error) -> DomainError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return payload_not_found(path);
    }

    DomainError::InternalError(format!(
        "Failed to read chat payload metadata {:?}: {}",
        path, error
    ))
}

pub(super) async fn open_existing_payload_file(path: &Path) -> Result<File, DomainError> {
    File::open(path)
        .await
        .map_err(|error| map_open_existing_error(path, error))
}

pub(super) async fn read_existing_payload_metadata(
    path: &Path,
) -> Result<std::fs::Metadata, DomainError> {
    fs::metadata(path)
        .await
        .map_err(|error| map_existing_metadata_error(path, error))
}

pub(super) fn file_signature_from_metadata(
    metadata: &std::fs::Metadata,
) -> Result<(u64, i64), DomainError> {
    let modified = metadata.modified().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read chat payload modified time: {}",
            error
        ))
    })?;
    let duration = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Chat payload modified time is before UNIX_EPOCH: {}",
                error
            ))
        })?;

    let modified_millis: i64 = duration.as_millis().try_into().map_err(|_| {
        DomainError::InternalError("Chat payload modified time overflows i64 millis".to_string())
    })?;

    Ok((metadata.len(), modified_millis))
}

pub(super) fn cursor_from_metadata(
    offset: u64,
    metadata: &std::fs::Metadata,
) -> Result<ChatPayloadCursor, DomainError> {
    let (size, modified_millis) = file_signature_from_metadata(metadata)?;
    Ok(ChatPayloadCursor {
        offset,
        size,
        modified_millis,
    })
}

pub(super) fn decode_jsonl_line_bytes(bytes: &[u8]) -> Result<String, DomainError> {
    let text = str::from_utf8(bytes).map_err(|error| {
        DomainError::InvalidData(format!("JSONL payload is not valid UTF-8: {}", error))
    })?;
    Ok(text.trim_end_matches(['\r', '\n']).to_string())
}

pub(super) async fn read_first_line_and_end_offset(
    path: &Path,
) -> Result<(String, u64), DomainError> {
    let mut file = open_existing_payload_file(path).await?;

    let mut buffer = [0u8; 8192];
    let mut bytes = Vec::new();
    let mut offset: u64 = 0;

    loop {
        let read = file.read(&mut buffer).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat payload header {:?}: {}",
                path, error
            ))
        })?;

        if read == 0 {
            if bytes.is_empty() {
                return Err(DomainError::InvalidData("Empty JSONL file".to_string()));
            }

            let line = decode_jsonl_line_bytes(&bytes)?;
            if line.trim().is_empty() {
                return Err(DomainError::InvalidData(
                    "Chat payload header line is empty".to_string(),
                ));
            }
            return Ok((line, offset));
        }

        if let Some(newline_pos) = buffer[..read].iter().position(|&value| value == b'\n') {
            bytes.extend_from_slice(&buffer[..newline_pos]);
            offset += (newline_pos + 1) as u64;

            let line = decode_jsonl_line_bytes(&bytes)?;
            if line.trim().is_empty() {
                return Err(DomainError::InvalidData(
                    "Chat payload header line is empty".to_string(),
                ));
            }
            return Ok((line, offset));
        }

        bytes.extend_from_slice(&buffer[..read]);
        offset += read as u64;
    }
}

pub(super) fn extract_integrity_slug_from_header_line(
    line: &str,
) -> Result<Option<String>, DomainError> {
    let header: Value = serde_json::from_str(line).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to parse chat payload header JSON: {}",
            error
        ))
    })?;

    Ok(header
        .get("chat_metadata")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("integrity"))
        .and_then(Value::as_str)
        .map(ToString::to_string))
}

pub(super) async fn ensure_parent_dir(path: &Path) -> Result<(), DomainError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat payload directory {:?}: {}",
                parent, error
            ))
        })?;
    }

    Ok(())
}

pub(super) async fn write_jsonl_lines_to_file(
    file: &mut File,
    first_line: &str,
    lines: &[String],
) -> Result<(), DomainError> {
    if first_line.trim().is_empty() {
        return Err(DomainError::InvalidData(
            "Chat payload header line is empty".to_string(),
        ));
    }

    file.write_all(first_line.as_bytes())
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to write chat payload header: {}", error))
        })?;
    file.write_all(b"\n").await.map_err(|error| {
        DomainError::InternalError(format!("Failed to write chat payload header: {}", error))
    })?;

    let mut first = true;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        if first {
            first = false;
        } else {
            file.write_all(b"\n").await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write chat payload newline: {}",
                    error
                ))
            })?;
        }

        file.write_all(line.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to write chat payload line: {}", error))
        })?;
    }

    Ok(())
}

pub(super) async fn write_jsonl_lines_at_end(
    file: &mut File,
    lines: &[String],
) -> Result<(), DomainError> {
    let mut first = true;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        if first {
            first = false;
        } else {
            file.write_all(b"\n").await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write chat payload newline: {}",
                    error
                ))
            })?;
        }

        file.write_all(line.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to write chat payload line: {}", error))
        })?;
    }

    Ok(())
}

pub(super) async fn replace_file(temp_path: &Path, target_path: &Path) -> Result<(), DomainError> {
    replace_file_with_fallback(temp_path, target_path).await
}

pub(super) fn verify_cursor_signature(
    path: &Path,
    cursor: ChatPayloadCursor,
    metadata: &std::fs::Metadata,
) -> Result<(), DomainError> {
    let (size, modified_millis) = file_signature_from_metadata(metadata)?;
    if cursor.size != size || cursor.modified_millis != modified_millis {
        return Err(DomainError::InvalidData(format!(
            "Cursor signature mismatch for {:?}",
            path
        )));
    }

    Ok(())
}

/// Count the number of JSONL lines between `start_offset` and `end_offset`.
pub(super) async fn count_lines_in_region(
    path: &Path,
    start_offset: u64,
    end_offset: u64,
) -> Result<usize, DomainError> {
    if end_offset <= start_offset {
        return Ok(0);
    }

    let mut file = open_existing_payload_file(path).await?;
    file.seek(SeekFrom::Start(start_offset))
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to seek chat payload file {:?}: {}",
                path, error
            ))
        })?;

    let region_len = (end_offset - start_offset) as usize;
    let mut remaining = region_len;
    let mut line_count: usize = 0;
    let mut buf = vec![0u8; WINDOW_READ_CHUNK_BYTES.min(region_len)];

    while remaining > 0 {
        let to_read = buf.len().min(remaining);
        let n = file.read(&mut buf[..to_read]).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat payload file {:?}: {}",
                path, error
            ))
        })?;
        if n == 0 {
            break;
        }
        line_count += buf[..n].iter().filter(|&&b| b == b'\n').count();
        remaining -= n;
    }

    // The last line may not end with \n, count it if there's content
    if region_len > 0 {
        line_count += 1;
        // But if the region ends with \n, we already counted it
        if remaining == 0 {
            let mut last_byte = [0u8; 1];
            file.seek(SeekFrom::Start(end_offset - 1))
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to seek chat payload file {:?}: {}",
                        path, error
                    ))
                })?;
            let n = file.read(&mut last_byte).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat payload file {:?}: {}",
                    path, error
                ))
            })?;
            if n == 1 && last_byte[0] == b'\n' {
                line_count -= 1;
            }
        }
    }

    Ok(line_count)
}

/// Verify the window baseline contract: the caller declares how many message
/// lines its last successful load/save left between cursor.offset and EOF,
/// and the write is rejected unless the file still matches exactly. Catches
/// stale cursors (mode switch, concurrent writer) without the false
/// accepts/rejects of a tolerance heuristic.
pub(super) async fn verify_window_baseline(
    path: &Path,
    cursor_offset: u64,
    file_size: u64,
    expected_window_line_count: usize,
) -> Result<(), DomainError> {
    let window_lines_on_disk = count_lines_in_region(path, cursor_offset, file_size).await?;

    if window_lines_on_disk != expected_window_line_count {
        return Err(DomainError::InvalidData(format!(
            "Window baseline mismatch for {:?}: expected {} message lines after the \
             cursor but found {} on disk. The cursor is stale (mode switch or \
             concurrent write). Reload the chat and retry.",
            path, expected_window_line_count, window_lines_on_disk
        )));
    }

    Ok(())
}

pub(super) async fn verify_cursor_offset_is_line_boundary(
    path: &Path,
    cursor_offset: u64,
) -> Result<(), DomainError> {
    if cursor_offset == 0 {
        return Ok(());
    }

    let mut file = open_existing_payload_file(path).await?;

    file.seek(SeekFrom::Start(cursor_offset.saturating_sub(1)))
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to seek chat payload file {:?}: {}",
                path, error
            ))
        })?;

    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read chat payload file {:?}: {}",
            path, error
        ))
    })?;

    if byte[0] != b'\n' {
        return Err(DomainError::InvalidData(format!(
            "Cursor offset is not at a JSONL line boundary for {:?}",
            path
        )));
    }

    Ok(())
}
