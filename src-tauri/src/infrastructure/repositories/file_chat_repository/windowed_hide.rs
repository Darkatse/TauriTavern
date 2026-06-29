use std::path::PathBuf;

use serde_json::Value;
use tokio::fs::File;
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_repository::ChatPayloadCursor;

use super::FileChatRepository;
use super::windowed_payload_io::{
    cursor_from_metadata, decode_jsonl_line_bytes, open_existing_payload_file,
    read_existing_payload_metadata, read_first_line_and_end_offset, replace_file,
    verify_cursor_offset_is_line_boundary, verify_cursor_signature, verify_window_baseline,
};

impl FileChatRepository {
    pub(super) async fn hide_character_payload_before_cursor(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        hide: bool,
        name_filter: Option<String>,
        expected_window_line_count: usize,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.ensure_directory_exists().await?;

        let path = self
            .resolve_character_chat_path(character_name, file_name)
            .await?;
        let backup_key = self.get_cache_key(character_name, file_name)?;

        let _write_guard = self.acquire_payload_write_lock(&path).await;
        let result = hide_payload_before_cursor_internal(
            &path,
            cursor,
            hide,
            name_filter.as_deref(),
            expected_window_line_count,
        )
        .await?;

        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(&backup_key);
        }
        self.remove_summary_cache_for_path(&path).await;

        self.backup_chat_file(&path, character_name, &backup_key)
            .await?;

        Ok(result)
    }

    pub(super) async fn hide_group_payload_before_cursor(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        hide: bool,
        name_filter: Option<String>,
        expected_window_line_count: usize,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.ensure_directory_exists().await?;

        let path = self.get_group_chat_path(chat_id)?;
        let _write_guard = self.acquire_payload_write_lock(&path).await;
        let backup_key = Self::get_group_backup_key(chat_id)?;
        let result = hide_payload_before_cursor_internal(
            &path,
            cursor,
            hide,
            name_filter.as_deref(),
            expected_window_line_count,
        )
        .await?;

        self.remove_summary_cache_for_path(&path).await;
        self.backup_chat_file(&path, chat_id, &backup_key).await?;

        Ok(result)
    }
}

/// Rewrite a single JSONL message line with the target `is_system` value.
/// Returns None when the line should be copied verbatim (already in the
/// target state or filtered out by name).
fn rewrite_hidden_line(
    path: &PathBuf,
    line_bytes: &[u8],
    hide: bool,
    name_filter: Option<&str>,
) -> Result<Option<String>, DomainError> {
    let line = decode_jsonl_line_bytes(line_bytes)?;
    if line.trim().is_empty() {
        return Ok(None);
    }

    let mut value: Value = serde_json::from_str(&line).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to parse chat payload line in {:?}: {}",
            path, error
        ))
    })?;

    // The chat payload format is object-per-line JSONL; a non-object line is
    // corruption and full-mode loads would choke on it. Fail loudly instead
    // of carrying the bad line forward (the temp file is discarded, the
    // original is untouched).
    let Some(object) = value.as_object_mut() else {
        return Err(DomainError::InvalidData(format!(
            "Chat payload line is not an object in {:?}",
            path
        )));
    };

    if let Some(filter) = name_filter {
        let name = object.get("name").and_then(Value::as_str).unwrap_or("");
        if name != filter {
            return Ok(None);
        }
    }

    let current = object
        .get("is_system")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if current == hide {
        return Ok(None);
    }

    object.insert("is_system".to_string(), Value::Bool(hide));

    serde_json::to_string(&value).map(Some).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to serialize chat payload line for {:?}: {}",
            path, error
        ))
    })
}

async fn hide_payload_before_cursor_internal(
    path: &PathBuf,
    cursor: ChatPayloadCursor,
    hide: bool,
    name_filter: Option<&str>,
    expected_window_line_count: usize,
) -> Result<ChatPayloadCursor, DomainError> {
    let metadata = read_existing_payload_metadata(path).await?;
    verify_cursor_signature(path, cursor, &metadata)?;

    if cursor.offset > metadata.len() {
        return Err(DomainError::InvalidData(format!(
            "Cursor offset is out of bounds for {:?}",
            path
        )));
    }

    let (_existing_header, header_end_offset) = read_first_line_and_end_offset(path).await?;

    if cursor.offset < header_end_offset {
        return Err(DomainError::InvalidData(format!(
            "Cursor offset is before chat payload body for {:?}",
            path
        )));
    }

    verify_cursor_offset_is_line_boundary(path, cursor.offset).await?;

    // Window baseline contract: same anchor validation as the windowed
    // save/patch paths — a stale cursor must be rejected before any rewrite.
    verify_window_baseline(
        path,
        cursor.offset,
        metadata.len(),
        expected_window_line_count,
    )
    .await?;

    if cursor.offset == header_end_offset {
        return cursor_from_metadata(cursor.offset, &metadata);
    }

    let temp_path = FileChatRepository::temp_payload_path(path);
    let mut out = File::create(&temp_path).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create chat payload file {:?}: {}",
            temp_path, error
        ))
    })?;

    let mut source = open_existing_payload_file(path).await?;

    {
        let mut header_region = (&mut source).take(header_end_offset);
        io::copy(&mut header_region, &mut out)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to copy chat payload file {:?}: {}",
                    path, error
                ))
            })?;
    }

    let mut new_cursor_offset = header_end_offset;
    {
        let body_len = cursor.offset - header_end_offset;
        let mut region = BufReader::new((&mut source).take(body_len));
        let mut line_bytes: Vec<u8> = Vec::new();

        loop {
            line_bytes.clear();
            let read = region
                .read_until(b'\n', &mut line_bytes)
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to read chat payload file {:?}: {}",
                        path, error
                    ))
                })?;

            if read == 0 {
                break;
            }

            match rewrite_hidden_line(path, &line_bytes, hide, name_filter)? {
                Some(rewritten) => {
                    out.write_all(rewritten.as_bytes()).await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write chat payload line: {}",
                            error
                        ))
                    })?;
                    out.write_all(b"\n").await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write chat payload newline: {}",
                            error
                        ))
                    })?;
                    new_cursor_offset += rewritten.as_bytes().len() as u64 + 1;
                }
                None => {
                    out.write_all(&line_bytes).await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write chat payload line: {}",
                            error
                        ))
                    })?;
                    new_cursor_offset += line_bytes.len() as u64;
                }
            }
        }
    }

    io::copy(&mut source, &mut out).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to copy chat payload file {:?}: {}",
            path, error
        ))
    })?;

    out.flush().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to flush chat payload file: {}", error))
    })?;

    replace_file(&temp_path, path).await?;

    tracing::debug!(
        "Rewrote hidden state before cursor in chat payload: {:?}",
        path
    );

    let metadata = read_existing_payload_metadata(path).await?;
    cursor_from_metadata(new_cursor_offset, &metadata)
}
