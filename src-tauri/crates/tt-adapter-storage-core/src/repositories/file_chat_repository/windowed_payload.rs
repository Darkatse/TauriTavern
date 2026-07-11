use std::path::Path;
use std::str;

use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_repository::{
    ChatPayloadChunk, ChatPayloadCursor, ChatPayloadTail,
};

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
        let path = self
            .resolve_character_chat_path(character_name, file_name)
            .await?;
        read_payload_tail_lines(&path, max_lines).await
    }

    pub(super) async fn get_character_payload_before_lines(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError> {
        let path = self
            .resolve_character_chat_path(character_name, file_name)
            .await?;
        read_payload_before_lines(&path, cursor, max_lines).await
    }

    pub(super) async fn get_group_payload_tail_lines(
        &self,
        chat_id: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, DomainError> {
        let path = self.get_group_chat_path(chat_id)?;
        read_payload_tail_lines(&path, max_lines).await
    }

    pub(super) async fn get_group_payload_before_lines(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError> {
        let path = self.get_group_chat_path(chat_id)?;
        read_payload_before_lines(&path, cursor, max_lines).await
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
