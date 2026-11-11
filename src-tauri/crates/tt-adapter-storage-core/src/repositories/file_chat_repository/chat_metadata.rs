use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;

use indexmap::IndexMap;
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::{Value, value::RawValue};

use crate::chat_jsonl::{
    parse_header, parse_header_integrity, parse_metadata_integrity, read_header_record,
    read_header_record_async,
};
use crate::file_system::persist_file_blocking;
use tt_domain::errors::DomainError;

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
    ) -> Result<Value, DomainError> {
        self.read_optional_chat_metadata_from_path(path)
            .await?
            .filter(Value::is_object)
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Chat header {} requires a chat_metadata object",
                    path.display()
                ))
            })
    }

    pub(super) async fn read_optional_chat_metadata_from_path(
        &self,
        path: &Path,
    ) -> Result<Option<Value>, DomainError> {
        let Some(header) = read_header_bytes(path).await? else {
            return Ok(None);
        };
        Ok(parse_header(&header)
            .map_err(|error| header_error(path, error))?
            .remove("chat_metadata"))
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

type Fields<'a> = IndexMap<String, &'a RawValue>;

fn fields<'a>(text: &'a str, label: &str) -> Result<Fields<'a>, DomainError> {
    serde_json::from_str(text)
        .map_err(|error| DomainError::InvalidData(format!("Invalid {label} object: {error}")))
}

/// A serialization view: replace one field without copying the other JSON values.
struct WithField<'a, T> {
    fields: &'a Fields<'a>,
    key: &'a str,
    value: T,
}

impl<T: Serialize> Serialize for WithField<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (key, value) in self.fields {
            if key != self.key {
                map.serialize_entry(key, value)?;
            }
        }
        map.serialize_entry(self.key, &self.value)?;
        map.end()
    }
}

fn write_header(
    out: &mut impl Write,
    header: &Fields<'_>,
    value: &RawValue,
    namespace: Option<&str>,
) -> Result<(), DomainError> {
    let existing_metadata = header.get("chat_metadata");
    let existing_integrity = existing_metadata
        .map(|metadata| parse_metadata_integrity(metadata))
        .transpose()?
        .flatten();
    let result = if let Some(namespace) = namespace {
        let metadata = fields(
            existing_metadata
                .ok_or_else(|| {
                    DomainError::InvalidData("Chat header requires a chat_metadata object".into())
                })?
                .get(),
            "chat_metadata",
        )?;
        let mut extensions = match metadata.get("extensions") {
            Some(value) => fields(value.get(), "chat_metadata.extensions")?,
            None => Fields::new(),
        };
        if value.get() == "null" {
            extensions.shift_remove(namespace);
        } else {
            extensions.insert(namespace.to_owned(), value);
        }
        let metadata = WithField {
            fields: &metadata,
            key: "extensions",
            value: &extensions,
        };
        serde_json::to_writer(
            out,
            &WithField {
                fields: header,
                key: "chat_metadata",
                value: metadata,
            },
        )
    } else {
        if !value.get().starts_with('{') {
            return Err(DomainError::InvalidData(
                "Chat metadata must be a JSON object".into(),
            ));
        }
        let incoming_integrity = parse_metadata_integrity(value)?;
        verify_integrity_match(existing_integrity.as_deref(), incoming_integrity.as_deref())?;
        serde_json::to_writer(
            out,
            &WithField {
                fields: header,
                key: "chat_metadata",
                value,
            },
        )
    };
    result.map_err(|error| {
        DomainError::InternalError(format!("Failed to write chat header: {error}"))
    })
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
    let header_text = std::str::from_utf8(&header_line)
        .map_err(|error| DomainError::InvalidData(format!("Invalid chat header UTF-8: {error}")))?;
    let header = fields(header_text, "chat header").map_err(|error| header_error(path, error))?;
    let incoming = fs::read_to_string(stage_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read metadata stage {}: {error}",
            stage_path.display()
        ))
    })?;
    // Parse the entire staged document, including its end; never ignore trailing data.
    let value: &RawValue = serde_json::from_str(&incoming).map_err(|error| {
        DomainError::InvalidData(format!("Invalid metadata commit JSON: {error}"))
    })?;
    // Raw values are copied into one JSONL header, so internal line breaks cannot be published.
    if value.get().contains(['\r', '\n']) {
        return Err(DomainError::InvalidData(
            "Metadata commit must be a single JSONL record".into(),
        ));
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
    write_header(&mut out, &header, value, namespace)?;
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
