use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::Path;

use serde_json::{Map, Value};

use crate::chat_jsonl::{
    parse_header, read_header_record, read_header_record_async, validate_metadata_integrity,
};
use crate::file_system::persist_file_blocking;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_payload_commit_repository::ChatPayloadTarget;

use super::FileChatRepository;
use super::integrity::verify_integrity_match;
use super::windowed_payload_io::{map_open_existing_error, open_existing_payload_file};

impl FileChatRepository {
    pub(super) async fn read_chat_metadata_from_path(
        &self,
        path: &Path,
    ) -> Result<Value, DomainError> {
        let mut reader = tokio::io::BufReader::new(open_existing_payload_file(path).await?);
        let (header, _) = read_header_record_async(&mut reader)
            .await?
            .ok_or_else(|| {
                DomainError::InvalidData("Cannot read metadata from a chat without a header".into())
            })?;
        parse_header(&header)?
            .remove("chat_metadata")
            .filter(Value::is_object)
            .ok_or_else(|| {
                DomainError::InvalidData("Chat header requires a chat_metadata object".into())
            })
    }

    pub(super) async fn replace_chat_metadata(
        &self,
        target: ChatPayloadTarget,
        chat_metadata: Value,
    ) -> Result<(), DomainError> {
        if !chat_metadata.is_object() {
            return Err(DomainError::InvalidData(
                "Chat metadata must be a JSON object".into(),
            ));
        }
        self.rewrite_chat_header(&target, move |header| {
            let incoming = validate_metadata_integrity(&chat_metadata)?;
            let existing = header
                .get("chat_metadata")
                .and_then(|metadata| metadata.get("integrity"))
                .and_then(Value::as_str);
            verify_integrity_match(existing, incoming)?;
            header.insert("chat_metadata".into(), chat_metadata);
            Ok(())
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
            let metadata = header
                .get_mut("chat_metadata")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    DomainError::InvalidData("Chat header requires a chat_metadata object".into())
                })?;
            let extensions = metadata
                .entry("extensions")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| {
                    DomainError::InvalidData(
                        "chat_metadata.extensions must be a JSON object".into(),
                    )
                })?;
            if value.is_null() {
                extensions.remove(&namespace);
            } else {
                extensions.insert(namespace, value);
            }
            Ok(())
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
    let (header_line, _) = read_header_record(&mut source)?.ok_or_else(|| {
        DomainError::InvalidData("Cannot update metadata in a chat without a header".into())
    })?;
    let mut header = parse_header(&header_line)?;
    edit(&mut header)?;

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
        serde_json::to_writer(&mut out, &header)
            .map_err(io::Error::other)
            .and_then(|()| out.write_all(b"\n"))
            .map_err(|error| {
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
