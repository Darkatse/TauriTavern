use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;

use serde_json::value::RawValue;

use crate::chat_jsonl::{
    parse_header_integrity, parse_header_metadata, parse_metadata_integrity, read_header_record,
    read_header_record_async,
};
use crate::file_system::persist_file_blocking;
use tt_domain::errors::DomainError;
use tt_domain::json_document::JsonDocument;

use super::FileChatRepository;
use super::integrity::verify_integrity_match;
use super::windowed_payload_io::{map_open_existing_error, open_existing_payload_file};

impl FileChatRepository {
    pub(super) async fn read_chat_integrity_from_path(
        &self,
        path: &Path,
    ) -> Result<Option<String>, DomainError> {
        let Some(header) = read_header_bytes(path).await? else {
            return Ok(None);
        };
        parse_header_integrity(&header).map_err(|error| header_error(path, error))
    }

    pub(super) async fn read_chat_metadata_from_path(
        &self,
        path: &Path,
    ) -> Result<Box<RawValue>, DomainError> {
        self.read_optional_chat_metadata_from_path(path)
            .await?
            .filter(|metadata| is_object(metadata))
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Chat header {} requires a chat_metadata object",
                    path.display()
                ))
            })
    }

    /// Returns the stored text: decoding it into a `Value` would sort its keys.
    pub(super) async fn read_optional_chat_metadata_from_path(
        &self,
        path: &Path,
    ) -> Result<Option<Box<RawValue>>, DomainError> {
        let Some(header) = read_header_bytes(path).await? else {
            return Ok(None);
        };
        parse_header_metadata(&header)
            .map(|metadata| metadata.map(ToOwned::to_owned))
            .map_err(|error| header_error(path, error))
    }

    /// Apply a staged metadata value to the latest target while owning its mutation lock.
    pub(super) async fn commit_metadata_stage(
        &self,
        path: &Path,
        stage_path: &Path,
        publish_path: &Path,
        namespace: Option<String>,
    ) -> Result<(), DomainError> {
        let write_guard = self.acquire_payload_mutation_lock(path).await;
        let path = path.to_owned();
        let stage_path = stage_path.to_owned();
        let publish_path = publish_path.to_owned();
        tokio::task::spawn_blocking(move || {
            // The writer owns the lock even if its async caller stops waiting.
            let _write_guard = write_guard;
            rewrite_chat_header_file(&path, &stage_path, &publish_path, namespace.as_deref())
        })
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Chat header update task failed: {error}"))
        })?
    }
}

async fn read_header_bytes(path: &Path) -> Result<Option<Vec<u8>>, DomainError> {
    let mut reader = tokio::io::BufReader::new(open_existing_payload_file(path).await?);
    read_header_record_async(&mut reader)
        .await
        .map(|record| record.map(|(bytes, _)| bytes))
        .map_err(|error| header_error(path, error))
}

// Attach the file to the underlying message, not to an already formatted error.
fn header_error(path: &Path, error: DomainError) -> DomainError {
    match error {
        DomainError::InvalidData(message) => {
            DomainError::InvalidData(format!("Chat header {}: {message}", path.display()))
        }
        DomainError::InternalError(message) => {
            DomainError::InternalError(format!("Chat header {}: {message}", path.display()))
        }
        other => other,
    }
}

fn is_object(value: &RawValue) -> bool {
    value.get().starts_with('{')
}

fn invalid_metadata(error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid chat metadata: {error}"))
}

/// The header as a document, with its identity read the way every reader reads it.
fn editable_header(line: &[u8]) -> Result<(JsonDocument, Option<String>), DomainError> {
    let integrity = parse_header_integrity(line)?;
    let header = JsonDocument::parse(line).map_err(|error| {
        DomainError::InvalidData(format!("Invalid chat header object: {error}"))
    })?;
    Ok((header, integrity))
}

fn replace_metadata(
    header: &mut JsonDocument,
    existing_integrity: Option<&str>,
    metadata: Box<RawValue>,
) -> Result<(), DomainError> {
    if !is_object(&metadata) {
        return Err(DomainError::InvalidData(
            "Chat metadata must be a JSON object".into(),
        ));
    }
    let incoming_integrity = parse_metadata_integrity(&metadata)?;
    verify_integrity_match(existing_integrity, incoming_integrity.as_deref())?;
    header.set("chat_metadata", metadata);
    Ok(())
}

fn set_metadata_extension(
    header: &mut JsonDocument,
    namespace: &str,
    value: Box<RawValue>,
) -> Result<(), DomainError> {
    let extensions = header
        .object_mut(&["chat_metadata"])
        .map_err(invalid_metadata)?
        .ok_or_else(|| {
            DomainError::InvalidData("Chat header requires a chat_metadata object".into())
        })?
        .object_entry(&["extensions"])
        .map_err(invalid_metadata)?;
    if value.get() == "null" {
        extensions.remove(namespace);
    } else {
        extensions.set(namespace, value);
    }
    Ok(())
}

fn rewrite_chat_header_file(
    path: &Path,
    stage_path: &Path,
    publish_path: &Path,
    namespace: Option<&str>,
) -> Result<(), DomainError> {
    let source = File::open(path).map_err(|error| map_open_existing_error(path, error))?;
    let mut source = BufReader::new(source);
    let (header_line, _) = read_header_record(&mut source)
        .map_err(|error| header_error(path, error))?
        .ok_or_else(|| {
            DomainError::InvalidData("Cannot update metadata in a chat without a header".into())
        })?;
    let (mut header, existing_integrity) =
        editable_header(&header_line).map_err(|error| header_error(path, error))?;
    let incoming = fs::read_to_string(stage_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read metadata stage {}: {error}",
            stage_path.display()
        ))
    })?;
    // Parse the entire staged document, including its end; never ignore trailing data.
    let value = RawValue::from_string(incoming).map_err(|error| {
        DomainError::InvalidData(format!("Invalid metadata commit JSON: {error}"))
    })?;
    // Raw values are copied into one JSONL header, so internal line breaks cannot be published.
    if value.get().contains(['\r', '\n']) {
        return Err(DomainError::InvalidData(
            "Metadata commit must be a single JSONL record".into(),
        ));
    }
    match namespace {
        Some(namespace) => set_metadata_extension(&mut header, namespace, value)?,
        None => replace_metadata(&mut header, existing_integrity.as_deref(), value)?,
    }

    let mut out = BufWriter::new(
        File::options()
            .write(true)
            .create_new(true)
            .open(publish_path)
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat publication stage {}: {error}",
                    publish_path.display()
                ))
            })?,
    );
    serde_json::to_writer(&mut out, &header).map_err(|error| {
        DomainError::InternalError(format!("Failed to write chat header: {error}"))
    })?;
    out.write_all(b"\n").map_err(io_error)?;
    // Keep the same reader: it may already hold buffered body bytes.
    io::copy(&mut source, &mut out).map_err(io_error)?;
    drop(source);
    let out = out
        .into_inner()
        .map_err(|error| io_error(error.into_error()))?;
    persist_file_blocking(out, publish_path, path)
}

fn io_error(error: io::Error) -> DomainError {
    DomainError::InternalError(format!("Chat metadata write failed: {error}"))
}
