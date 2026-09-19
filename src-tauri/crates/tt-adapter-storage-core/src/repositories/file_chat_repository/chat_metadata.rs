use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use serde_json::{Map, Value};

use crate::file_system::persist_file_blocking;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_payload_commit_repository::ChatPayloadTarget;

use super::FileChatRepository;
use super::integrity::verify_integrity_match;
use super::windowed_payload_io::{
    decode_jsonl_line_bytes, map_open_existing_error, read_first_line_and_end_offset,
};

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

fn apply_chat_metadata_replacement(
    header_map: &mut Map<String, Value>,
    chat_metadata: Map<String, Value>,
) -> Result<(), DomainError> {
    let existing = header_map
        .get("chat_metadata")
        .and_then(|metadata| metadata.get("integrity"))
        .and_then(Value::as_str);
    let incoming = chat_metadata.get("integrity").and_then(Value::as_str);
    verify_integrity_match(existing, incoming)?;
    header_map.insert("chat_metadata".to_string(), Value::Object(chat_metadata));
    Ok(())
}

fn apply_metadata_extension_update(
    header_map: &mut Map<String, Value>,
    namespace: &str,
    value: Value,
) -> Result<(), DomainError> {
    let meta_value = header_map.get_mut("chat_metadata").ok_or_else(|| {
        DomainError::InvalidData("Chat header is missing chat_metadata".to_string())
    })?;

    let meta_map = meta_value.as_object_mut().ok_or_else(|| {
        DomainError::InvalidData("chat_metadata is not a JSON object".to_string())
    })?;

    let extensions_value = meta_map
        .entry("extensions".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let extensions_map = extensions_value.as_object_mut().ok_or_else(|| {
        DomainError::InvalidData("chat_metadata.extensions is not a JSON object".to_string())
    })?;

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
        let meta = header_map.get("chat_metadata").cloned().ok_or_else(|| {
            DomainError::InvalidData("Chat header is missing chat_metadata".to_string())
        })?;
        ensure_object(meta, "chat_metadata").map(Value::Object)
    }

    pub(super) async fn replace_chat_metadata(
        &self,
        target: ChatPayloadTarget,
        chat_metadata: Value,
    ) -> Result<(), DomainError> {
        let chat_metadata = ensure_object(chat_metadata, "Chat metadata")?;
        self.rewrite_chat_header(&target, move |header| {
            apply_chat_metadata_replacement(header, chat_metadata)
        })
        .await
    }

    pub(super) async fn set_chat_metadata_extension(
        &self,
        target: ChatPayloadTarget,
        namespace: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let namespace = namespace.to_owned();
        self.rewrite_chat_header(&target, move |header| {
            apply_metadata_extension_update(header, &namespace, value)
        })
        .await
    }

    pub(super) async fn rewrite_chat_header(
        &self,
        target: &ChatPayloadTarget,
        edit: impl FnOnce(&mut Map<String, Value>) -> Result<(), DomainError> + Send + 'static,
    ) -> Result<(), DomainError> {
        let path = self.resolve_chat_commit_target(target).await?;
        let write_guard = self.acquire_payload_mutation_lock(&path).await;
        let write_path = path.clone();
        tokio::task::spawn_blocking(move || {
            // The writer owns the lock even if its async caller stops waiting.
            let _write_guard = write_guard;
            rewrite_chat_header_file(&write_path, edit)
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Chat header update task failed for {:?}: {}",
                path, error
            ))
        })??;
        self.invalidate_chat_caches(target, &path).await
    }
}

fn rewrite_chat_header_file(
    path: &Path,
    edit: impl FnOnce(&mut Map<String, Value>) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    let source = File::open(path).map_err(|error| map_open_existing_error(path, error))?;
    let mut source = BufReader::new(source);
    let mut header_bytes = Vec::new();
    source
        .read_until(b'\n', &mut header_bytes)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to read chat header {:?}: {}", path, error))
        })?;
    let header_line = decode_jsonl_line_bytes(&header_bytes)?;
    let mut header = ensure_object(parse_header_json(&header_line)?, "Chat header")?;
    edit(&mut header)?;
    let serialized = serialize_header_json(&Value::Object(header))?;

    let temp_path = FileChatRepository::temp_payload_path(path);
    let mut out = File::options()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat payload temp file {:?}: {}",
                temp_path, error
            ))
        })?;
    let result = (|| {
        writeln!(out, "{serialized}").map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write chat header {:?}: {}",
                temp_path, error
            ))
        })?;
        // Keep the same reader: it may already hold buffered body bytes.
        io::copy(&mut source, &mut out).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to copy chat payload body {:?}: {}",
                path, error
            ))
        })?;
        drop(source);
        persist_file_blocking(out, &temp_path, path)
    })();

    if let Err(error) = &result
        && let Err(cleanup_error) = fs::remove_file(&temp_path)
    {
        return Err(DomainError::InternalError(format!(
            "{}; failed to remove chat payload temp file {:?}: {}",
            error, temp_path, cleanup_error
        )));
    }
    result
}
