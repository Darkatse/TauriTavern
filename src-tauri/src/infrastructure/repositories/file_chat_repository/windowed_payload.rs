use std::path::{Path, PathBuf};
use std::str;

use serde_json::Value;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_repository::{
    ChatPayloadChunk, ChatPayloadCursor, ChatPayloadTail,
};
use crate::infrastructure::logging::logger;

use super::FileChatRepository;
use super::windowed_payload_io::*;

async fn read_tail_lines_with_offsets(
    path: &Path,
    start_bound: u64,
    end_position: u64,
    max_lines: usize,
) -> Result<Vec<(u64, String)>, DomainError> {
    if max_lines == 0 || end_position <= start_bound {
        return Ok(Vec::new());
    }

    let mut file = open_existing_payload_file(path).await?;

    let mut pos = end_position;
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    let mut newline_count: usize = 0;
    let mut blocks_start: u64 = pos;

    while pos > start_bound && newline_count <= max_lines {
        let available = pos - start_bound;
        let read_size = (available.min(WINDOW_READ_CHUNK_BYTES as u64)) as usize;

        pos -= read_size as u64;
        file.seek(SeekFrom::Start(pos)).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to seek chat payload file {:?}: {}",
                path, error
            ))
        })?;

        let mut buf = vec![0u8; read_size];
        file.read_exact(&mut buf).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat payload file {:?}: {}",
                path, error
            ))
        })?;

        newline_count += buf.iter().filter(|&&b| b == b'\n').count();
        blocks.push(buf);
        blocks_start = pos;
    }

    blocks.reverse();
    let total_size: usize = blocks.iter().map(|block| block.len()).sum();
    let mut data = Vec::with_capacity(total_size);
    for block in blocks {
        data.extend_from_slice(&block);
    }

    let mut raw_lines: Vec<(u64, &[u8])> = Vec::new();
    let mut line_start: usize = 0;
    for (index, &byte) in data.iter().enumerate() {
        if byte != b'\n' {
            continue;
        }

        let slice = &data[line_start..index];
        let offset = blocks_start + line_start as u64;
        raw_lines.push((offset, slice));
        line_start = index + 1;
    }

    if line_start < data.len() {
        let slice = &data[line_start..];
        let offset = blocks_start + line_start as u64;
        raw_lines.push((offset, slice));
    }

    if blocks_start > start_bound && !raw_lines.is_empty() {
        file.seek(SeekFrom::Start(blocks_start.saturating_sub(1)))
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

        let starts_on_line_boundary = byte[0] == b'\n';
        if !starts_on_line_boundary {
            raw_lines.remove(0);
        }
    }

    let mut lines: Vec<(u64, String)> = Vec::with_capacity(raw_lines.len());
    for (offset, bytes) in raw_lines {
        if bytes.is_empty() {
            return Err(DomainError::InvalidData(format!(
                "Chat payload contains empty JSONL line at offset {} for {:?}",
                offset, path
            )));
        }

        let text = str::from_utf8(bytes).map_err(|error| {
            DomainError::InvalidData(format!("JSONL payload is not valid UTF-8: {}", error))
        })?;
        let normalized = text.trim_end_matches('\r');
        if normalized.trim().is_empty() {
            return Err(DomainError::InvalidData(format!(
                "Chat payload contains blank JSONL line at offset {} for {:?}",
                offset, path
            )));
        }
        lines.push((offset, normalized.to_string()));
    }

    if lines.len() > max_lines {
        lines.drain(0..(lines.len() - max_lines));
    }

    Ok(lines)
}

impl FileChatRepository {
    pub(super) async fn get_character_payload_tail_lines(
        &self,
        character_name: &str,
        file_name: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, DomainError> {
        let path = self.get_chat_path(character_name, file_name);
        read_payload_tail_lines(&path, max_lines).await
    }

    pub(super) async fn get_character_payload_before_lines(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError> {
        let path = self.get_chat_path(character_name, file_name);
        read_payload_before_lines(&path, cursor, max_lines).await
    }

    pub(super) async fn save_character_payload_windowed(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        header: String,
        lines: Vec<String>,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.ensure_directory_exists().await?;

        let character_dir = self.get_character_dir(character_name);
        fs::create_dir_all(&character_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create character chat directory {:?}: {}",
                character_dir, error
            ))
        })?;

        let path = self.get_chat_path(character_name, file_name);
        let backup_key = self.get_cache_key(character_name, file_name);
        let result = save_payload_windowed_internal(&path, cursor, header, lines, force).await?;

        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(&backup_key);
        }
        self.remove_summary_cache_for_path(&path).await;

        self.backup_chat_file(&path, character_name, &backup_key)
            .await?;

        Ok(result)
    }

    pub(super) async fn get_group_payload_tail_lines(
        &self,
        chat_id: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, DomainError> {
        let path = self.get_group_chat_path(chat_id);
        read_payload_tail_lines(&path, max_lines).await
    }

    pub(super) async fn get_group_payload_before_lines(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError> {
        let path = self.get_group_chat_path(chat_id);
        read_payload_before_lines(&path, cursor, max_lines).await
    }

    pub(super) async fn save_group_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        lines: Vec<String>,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.ensure_directory_exists().await?;

        let path = self.get_group_chat_path(chat_id);
        let backup_key = format!("group:{}", Self::strip_jsonl_extension(chat_id));
        let result = save_payload_windowed_internal(&path, cursor, header, lines, force).await?;

        self.remove_summary_cache_for_path(&path).await;
        self.backup_chat_file(&path, chat_id, &backup_key).await?;

        Ok(result)
    }
}

async fn read_payload_tail_lines(
    path: &Path,
    max_lines: usize,
) -> Result<ChatPayloadTail, DomainError> {
    let metadata = read_existing_payload_metadata(path).await?;

    let (header, header_end_offset) = read_first_line_and_end_offset(path).await?;
    let end_position = metadata.len();

    let lines_with_offsets =
        read_tail_lines_with_offsets(path, header_end_offset, end_position, max_lines).await?;

    let cursor_offset = lines_with_offsets
        .first()
        .map(|(offset, _)| *offset)
        .unwrap_or(header_end_offset);

    Ok(ChatPayloadTail {
        header,
        lines: lines_with_offsets
            .into_iter()
            .map(|(_, line)| line)
            .collect(),
        cursor: cursor_from_metadata(cursor_offset, &metadata)?,
        has_more_before: cursor_offset > header_end_offset,
    })
}

async fn read_payload_before_lines(
    path: &Path,
    cursor: ChatPayloadCursor,
    max_lines: usize,
) -> Result<ChatPayloadChunk, DomainError> {
    let metadata = read_existing_payload_metadata(path).await?;
    verify_cursor_signature(path, cursor, &metadata)?;

    let (_, header_end_offset) = read_first_line_and_end_offset(path).await?;

    if cursor.offset > metadata.len() {
        return Err(DomainError::InvalidData(format!(
            "Cursor offset is out of bounds for {:?}",
            path
        )));
    }

    let end_position = cursor.offset;
    if end_position < header_end_offset {
        return Err(DomainError::InvalidData(format!(
            "Cursor offset is before chat payload body for {:?}",
            path
        )));
    }

    let lines_with_offsets =
        read_tail_lines_with_offsets(path, header_end_offset, end_position, max_lines).await?;

    let new_offset = lines_with_offsets
        .first()
        .map(|(offset, _)| *offset)
        .unwrap_or(header_end_offset);

    Ok(ChatPayloadChunk {
        lines: lines_with_offsets
            .into_iter()
            .map(|(_, line)| line)
            .collect(),
        cursor: cursor_from_metadata(new_offset, &metadata)?,
        has_more_before: new_offset > header_end_offset,
    })
}

async fn save_payload_windowed_internal(
    path: &PathBuf,
    cursor: ChatPayloadCursor,
    header: String,
    lines: Vec<String>,
    force: bool,
) -> Result<ChatPayloadCursor, DomainError> {
    let header_integrity = extract_integrity_slug_from_header_line(&header)?;
    let has_lines = lines.iter().any(|line| !line.trim().is_empty());

    let existing_metadata = match read_existing_payload_metadata(path).await {
        Ok(metadata) => Some(metadata),
        Err(DomainError::NotFound(_)) => None,
        Err(error) => return Err(error),
    };

    if existing_metadata.is_none() {
        ensure_parent_dir(path).await?;

        let temp_path = path.with_extension("jsonl.tmp");
        let mut file = File::create(&temp_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat payload file {:?}: {}",
                temp_path, error
            ))
        })?;

        write_jsonl_lines_to_file(&mut file, &header, &lines).await?;
        file.flush().await.map_err(|error| {
            DomainError::InternalError(format!("Failed to flush chat payload file: {}", error))
        })?;

        replace_file(&temp_path, path).await?;

        let metadata = read_existing_payload_metadata(path).await?;

        let header_end_offset = (header.as_bytes().len() + 1) as u64;
        return cursor_from_metadata(header_end_offset, &metadata);
    }

    let metadata = existing_metadata.unwrap();
    verify_cursor_signature(path, cursor, &metadata)?;

    let (existing_header, existing_header_end_offset) =
        read_first_line_and_end_offset(path).await?;
    let header_only = existing_header_end_offset == metadata.len();
    if cursor.offset > metadata.len() {
        return Err(DomainError::InvalidData(format!(
            "Cursor offset is out of bounds for {:?}",
            path
        )));
    }
    if cursor.offset < existing_header_end_offset {
        return Err(DomainError::InvalidData(format!(
            "Cursor offset is before chat payload body for {:?}",
            path
        )));
    }

    if !force {
        if let Some(incoming) = header_integrity {
            let existing = extract_integrity_slug_from_header_line(&existing_header)?;
            if let Some(existing) = existing {
                if existing != incoming {
                    return Err(DomainError::InvalidData("integrity".to_string()));
                }
            }
        }
    }

    let header_changed = match (
        serde_json::from_str::<Value>(&existing_header),
        serde_json::from_str::<Value>(&header),
    ) {
        (Ok(a), Ok(b)) => a != b,
        _ => existing_header != header,
    };

    if !header_changed {
        if !(header_only && cursor.offset == existing_header_end_offset) {
            verify_cursor_offset_is_line_boundary(path, cursor.offset).await?;
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .await
            .map_err(|error| map_open_existing_error(path, error))?;

        file.set_len(cursor.offset).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to truncate chat payload file {:?}: {}",
                path, error
            ))
        })?;

        let ends_with_newline = if cursor.offset == 0 {
            true
        } else {
            file.seek(SeekFrom::Start(cursor.offset.saturating_sub(1)))
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
            byte[0] == b'\n'
        };

        file.seek(SeekFrom::End(0)).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to seek chat payload file {:?}: {}",
                path, error
            ))
        })?;

        if has_lines && !ends_with_newline {
            if header_only && cursor.offset == existing_header_end_offset {
                file.write_all(b"\n").await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to write chat payload newline {:?}: {}",
                        path, error
                    ))
                })?;
            } else {
                return Err(DomainError::InvalidData(format!(
                    "Truncated chat payload does not end with newline for {:?}",
                    path
                )));
            }
        }

        write_jsonl_lines_at_end(&mut file, &lines).await?;
        file.flush().await.map_err(|error| {
            DomainError::InternalError(format!("Failed to flush chat payload file: {}", error))
        })?;
    } else {
        if !(header_only && cursor.offset == existing_header_end_offset) {
            verify_cursor_offset_is_line_boundary(path, cursor.offset).await?;
        }
        ensure_parent_dir(path).await?;

        let temp_path = path.with_extension("jsonl.tmp");
        let mut out = File::create(&temp_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat payload file {:?}: {}",
                temp_path, error
            ))
        })?;

        out.write_all(header.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to write chat payload header: {}", error))
        })?;
        out.write_all(b"\n").await.map_err(|error| {
            DomainError::InternalError(format!("Failed to write chat payload header: {}", error))
        })?;

        if cursor.offset > existing_header_end_offset {
            let mut source = open_existing_payload_file(path).await?;
            source
                .seek(SeekFrom::Start(existing_header_end_offset))
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to seek chat payload file {:?}: {}",
                        path, error
                    ))
                })?;

            let len = cursor.offset - existing_header_end_offset;
            let mut limited = source.take(len);
            io::copy(&mut limited, &mut out).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to copy chat payload file {:?}: {}",
                    path, error
                ))
            })?;
        }

        write_jsonl_lines_at_end(&mut out, &lines).await?;
        out.flush().await.map_err(|error| {
            DomainError::InternalError(format!("Failed to flush chat payload file: {}", error))
        })?;

        replace_file(&temp_path, path).await?;
    }

    logger::debug(&format!("Saved windowed chat payload: {:?}", path));

    let metadata = read_existing_payload_metadata(path).await?;

    let new_cursor_offset = match (header_changed, header_only, has_lines) {
        (true, _, _) => {
            let new_header_end_offset = (header.as_bytes().len() + 1) as u64;
            let preserved_prefix_bytes = cursor.offset.saturating_sub(existing_header_end_offset);
            new_header_end_offset + preserved_prefix_bytes
        }
        (false, true, true) => cursor.offset + 1,
        _ => cursor.offset,
    };

    cursor_from_metadata(new_cursor_offset, &metadata)
}
