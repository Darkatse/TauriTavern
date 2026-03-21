use std::path::Path;

use serde_json::{Map, Value};
use tokio::fs::File;
use tokio::io::{self, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::replace_file_with_fallback;

use super::windowed_payload_io::{open_existing_payload_file, read_first_line_and_end_offset};
use super::FileChatRepository;

fn ensure_object(value: Value, label: &str) -> Result<Map<String, Value>, DomainError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(DomainError::InvalidData(format!(
            "{} is not a JSON object",
            label
        ))),
    }
}

fn parse_header_json(header: &str) -> Result<Value, DomainError> {
    serde_json::from_str::<Value>(header).map_err(|error| {
        DomainError::InvalidData(format!("Failed to parse chat header JSON: {}", error))
    })
}

fn serialize_header_json(value: &Value) -> Result<String, DomainError> {
    serde_json::to_string(value).map_err(|error| {
        DomainError::InvalidData(format!("Failed to serialize chat header JSON: {}", error))
    })
}

fn apply_metadata_extension_update(
    header_value: &mut Value,
    namespace: &str,
    value: Value,
) -> Result<(), DomainError> {
    let header_map = header_value
        .as_object_mut()
        .ok_or_else(|| DomainError::InvalidData("Chat header is not a JSON object".to_string()))?;

    let meta_value = header_map
        .get_mut("chat_metadata")
        .ok_or_else(|| DomainError::InvalidData("Chat header is missing chat_metadata".to_string()))?;

    let meta_map = meta_value
        .as_object_mut()
        .ok_or_else(|| DomainError::InvalidData("chat_metadata is not a JSON object".to_string()))?;

    let extensions_value = meta_map
        .entry("extensions".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let extensions_map = extensions_value
        .as_object_mut()
        .ok_or_else(|| DomainError::InvalidData("chat_metadata.extensions is not a JSON object".to_string()))?;

    if value.is_null() {
        extensions_map.remove(namespace);
    } else {
        extensions_map.insert(namespace.to_string(), value);
    }

    Ok(())
}

impl FileChatRepository {
    pub(super) async fn read_chat_metadata_from_path(
        &self,
        path: &Path,
    ) -> Result<Value, DomainError> {
        let (header, _) = read_first_line_and_end_offset(path).await?;
        let header_value = parse_header_json(&header)?;
        let header_map = ensure_object(header_value, "Chat header")?;
        let meta = header_map
            .get("chat_metadata")
            .cloned()
            .ok_or_else(|| DomainError::InvalidData("Chat header is missing chat_metadata".to_string()))?;
        ensure_object(meta, "chat_metadata").map(Value::Object)
    }

    pub(super) async fn set_chat_metadata_extension_in_path(
        &self,
        path: &Path,
        namespace: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let _write_guard = self.acquire_payload_write_lock(path).await;

        let (header, header_end_offset) = read_first_line_and_end_offset(path).await?;
        let mut header_value = parse_header_json(&header)?;
        apply_metadata_extension_update(&mut header_value, namespace, value)?;
        let serialized = serialize_header_json(&header_value)?;

        let temp_path = Self::temp_payload_path(path);
        let mut out = File::create(&temp_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat payload temp file {:?}: {}",
                temp_path, error
            ))
        })?;

        out.write_all(serialized.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to write chat header: {}", error))
        })?;
        out.write_all(b"\n").await.map_err(|error| {
            DomainError::InternalError(format!("Failed to write chat header newline: {}", error))
        })?;

        let mut source = open_existing_payload_file(path).await?;
        source
            .seek(SeekFrom::Start(header_end_offset))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to seek chat payload file {:?}: {}",
                    path, error
                ))
            })?;

        io::copy(&mut source, &mut out).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to copy chat payload body {:?}: {}",
                path, error
            ))
        })?;

        out.flush().await.map_err(|error| {
            DomainError::InternalError(format!("Failed to flush chat payload file: {}", error))
        })?;

        replace_file_with_fallback(&temp_path, path).await?;
        self.remove_summary_cache_for_path(path).await;

        logger::debug(&format!(
            "Updated chat metadata extension for {:?}: {}",
            path, namespace
        ));

        Ok(())
    }
}
