use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_repository::{ChatPayloadCursor, ChatPayloadPatchOp};
use crate::infrastructure::logging::logger;

use super::FileChatRepository;
use super::windowed_payload_io::{
    WINDOW_READ_CHUNK_BYTES, cursor_from_metadata, ensure_parent_dir,
    extract_integrity_slug_from_header_line, map_open_existing_error, open_existing_payload_file,
    read_existing_payload_metadata, read_first_line_and_end_offset, replace_file,
    verify_cursor_offset_is_line_boundary, verify_cursor_signature, write_jsonl_lines_at_end,
    write_jsonl_lines_to_file,
};

impl FileChatRepository {
    pub(super) async fn patch_character_payload_windowed(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        header: String,
        op: ChatPayloadPatchOp,
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
        let result = patch_payload_windowed_internal(&path, cursor, header, op, force).await?;

        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(&backup_key);
        }
        self.remove_summary_cache_for_path(&path).await;

        self.backup_chat_file(&path, character_name, &backup_key)
            .await?;

        Ok(result)
    }

    pub(super) async fn patch_group_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        op: ChatPayloadPatchOp,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError> {
        self.ensure_directory_exists().await?;

        let path = self.get_group_chat_path(chat_id);
        let backup_key = format!("group:{}", Self::strip_jsonl_extension(chat_id));
        let result = patch_payload_windowed_internal(&path, cursor, header, op, force).await?;

        self.remove_summary_cache_for_path(&path).await;
        self.backup_chat_file(&path, chat_id, &backup_key).await?;

        Ok(result)
    }
}

async fn read_last_byte(path: &Path, len: u64) -> Result<Option<u8>, DomainError> {
    if len == 0 {
        return Ok(None);
    }

    let mut file = open_existing_payload_file(path).await?;
    file.seek(SeekFrom::Start(len.saturating_sub(1)))
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

    Ok(Some(byte[0]))
}

async fn find_line_start_offset_from_cursor(
    path: &Path,
    cursor_offset: u64,
    start_index: usize,
) -> Result<u64, DomainError> {
    if start_index == 0 {
        return Ok(cursor_offset);
    }

    let mut file = open_existing_payload_file(path).await?;
    file.seek(SeekFrom::Start(cursor_offset))
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to seek chat payload file {:?}: {}",
                path, error
            ))
        })?;

    let mut current_offset = cursor_offset;
    let mut remaining = start_index;
    let mut buffer = vec![0u8; WINDOW_READ_CHUNK_BYTES];

    loop {
        let read = file.read(&mut buffer).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat payload file {:?}: {}",
                path, error
            ))
        })?;

        if read == 0 {
            return Err(DomainError::InvalidData(format!(
                "Start index {} is out of bounds for {:?}",
                start_index, path
            )));
        }

        for (index, byte) in buffer[..read].iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }

            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                return Ok(current_offset + index as u64 + 1);
            }
        }

        current_offset += read as u64;
    }
}

async fn patch_payload_windowed_internal(
    path: &PathBuf,
    cursor: ChatPayloadCursor,
    header: String,
    op: ChatPayloadPatchOp,
    force: bool,
) -> Result<ChatPayloadCursor, DomainError> {
    let header_integrity = extract_integrity_slug_from_header_line(&header)?;

    let existing_metadata = match read_existing_payload_metadata(path).await {
        Ok(metadata) => Some(metadata),
        Err(DomainError::NotFound(_)) => None,
        Err(error) => return Err(error),
    };

    if existing_metadata.is_none() {
        ensure_parent_dir(path).await?;

        let (lines, start_index) = match op {
            ChatPayloadPatchOp::Append { lines } => (lines, 0usize),
            ChatPayloadPatchOp::RewriteFromIndex { start_index, lines } => (lines, start_index),
        };

        if start_index != 0 {
            return Err(DomainError::InvalidData(format!(
                "Start index {} is invalid for new chat payload {:?}",
                start_index, path
            )));
        }

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

    match op {
        ChatPayloadPatchOp::Append { lines } => {
            let has_lines = lines.iter().any(|line| !line.trim().is_empty());

            if !header_changed {
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .await
                    .map_err(|error| map_open_existing_error(path, error))?;

                let file_len = metadata.len();
                let ends_with_newline = match read_last_byte(path, file_len).await? {
                    Some(byte) => byte == b'\n',
                    None => true,
                };

                file.seek(SeekFrom::End(0)).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to seek chat payload file {:?}: {}",
                        path, error
                    ))
                })?;

                if has_lines && !ends_with_newline {
                    file.write_all(b"\n").await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write chat payload newline {:?}: {}",
                            path, error
                        ))
                    })?;
                }

                write_jsonl_lines_at_end(&mut file, &lines).await?;
                file.flush().await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to flush chat payload file: {}",
                        error
                    ))
                })?;
            } else {
                ensure_parent_dir(path).await?;

                let temp_path = path.with_extension("jsonl.tmp");
                let mut out = File::create(&temp_path).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create chat payload file {:?}: {}",
                        temp_path, error
                    ))
                })?;

                out.write_all(header.as_bytes()).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to write chat payload header: {}",
                        error
                    ))
                })?;
                out.write_all(b"\n").await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to write chat payload header: {}",
                        error
                    ))
                })?;

                if metadata.len() > existing_header_end_offset {
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

                    io::copy(&mut source, &mut out).await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to copy chat payload file {:?}: {}",
                            path, error
                        ))
                    })?;
                }

                let ends_with_newline = metadata.len() == existing_header_end_offset
                    || matches!(read_last_byte(path, metadata.len()).await?, Some(b'\n'));

                if has_lines && !ends_with_newline {
                    out.write_all(b"\n").await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write chat payload newline {:?}: {}",
                            path, error
                        ))
                    })?;
                }

                write_jsonl_lines_at_end(&mut out, &lines).await?;
                out.flush().await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to flush chat payload file: {}",
                        error
                    ))
                })?;

                replace_file(&temp_path, path).await?;
            }
        }
        ChatPayloadPatchOp::RewriteFromIndex { start_index, lines } => {
            verify_cursor_offset_is_line_boundary(path, cursor.offset).await?;

            let start_offset =
                find_line_start_offset_from_cursor(path, cursor.offset, start_index).await?;

            if start_offset > metadata.len() {
                return Err(DomainError::InvalidData(format!(
                    "Rewrite offset is out of bounds for {:?}",
                    path
                )));
            }

            let has_lines = lines.iter().any(|line| !line.trim().is_empty());

            if !header_changed {
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .await
                    .map_err(|error| map_open_existing_error(path, error))?;

                file.set_len(start_offset).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to truncate chat payload file {:?}: {}",
                        path, error
                    ))
                })?;

                file.seek(SeekFrom::End(0)).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to seek chat payload file {:?}: {}",
                        path, error
                    ))
                })?;

                let ends_with_newline = if start_offset == 0 {
                    true
                } else {
                    matches!(read_last_byte(path, start_offset).await?, Some(b'\n'))
                };

                if has_lines && !ends_with_newline {
                    file.write_all(b"\n").await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write chat payload newline {:?}: {}",
                            path, error
                        ))
                    })?;
                }

                write_jsonl_lines_at_end(&mut file, &lines).await?;
                file.flush().await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to flush chat payload file: {}",
                        error
                    ))
                })?;
            } else {
                ensure_parent_dir(path).await?;

                let temp_path = path.with_extension("jsonl.tmp");
                let mut out = File::create(&temp_path).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create chat payload file {:?}: {}",
                        temp_path, error
                    ))
                })?;

                out.write_all(header.as_bytes()).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to write chat payload header: {}",
                        error
                    ))
                })?;
                out.write_all(b"\n").await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to write chat payload header: {}",
                        error
                    ))
                })?;

                if start_offset > existing_header_end_offset {
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

                    let len = start_offset - existing_header_end_offset;
                    let mut limited = source.take(len);
                    io::copy(&mut limited, &mut out).await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to copy chat payload file {:?}: {}",
                            path, error
                        ))
                    })?;
                }

                let ends_with_newline = start_offset == existing_header_end_offset
                    || matches!(read_last_byte(path, start_offset).await?, Some(b'\n'));

                if has_lines && !ends_with_newline {
                    out.write_all(b"\n").await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write chat payload newline {:?}: {}",
                            path, error
                        ))
                    })?;
                }

                write_jsonl_lines_at_end(&mut out, &lines).await?;
                out.flush().await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to flush chat payload file: {}",
                        error
                    ))
                })?;

                replace_file(&temp_path, path).await?;
            }
        }
    }

    logger::debug(&format!("Patched windowed chat payload: {:?}", path));

    let metadata = read_existing_payload_metadata(path).await?;
    let new_cursor_offset = if header_changed {
        let new_header_end_offset = (header.as_bytes().len() + 1) as u64;
        let preserved_prefix_bytes = cursor.offset.saturating_sub(existing_header_end_offset);
        new_header_end_offset + preserved_prefix_bytes
    } else {
        cursor.offset
    };

    cursor_from_metadata(new_cursor_offset, &metadata)
}
