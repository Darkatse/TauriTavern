use std::path::Path;

use tokio::fs::{self, File};

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
