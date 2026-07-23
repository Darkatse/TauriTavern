use std::path::Path;
use std::str;

use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;

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
