//! Chat JSONL grammar. I/O callers retain their own read scope and original byte offsets.
use std::collections::HashMap;
use std::io::BufRead;
use std::ops::Range;
use std::path::Path;

use serde_json::{Map, Value, value::RawValue};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::file_system::{persist_file, unique_temp_path};
use tt_domain::errors::DomainError;

mod metadata;
pub(crate) use metadata::{HeaderMetadata, HeaderMetadataFields};

const BOM: &[u8] = b"\xef\xbb\xbf";

pub(crate) fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

pub(crate) fn trim_whitespace(bytes: &[u8]) -> &[u8] {
    &bytes[trim_whitespace_range(bytes)]
}

fn trim_whitespace_range(bytes: &[u8]) -> Range<usize> {
    let start = bytes
        .iter()
        .position(|&b| !is_whitespace(b))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| !is_whitespace(b))
        .map_or(start, |i| i + 1);
    start..end
}

/// One state per document, including BOM-only lines before the header.
#[derive(Default)]
pub(crate) struct RecordPrefix {
    bom_consumed: bool,
    record_seen: bool,
}

impl RecordPrefix {
    /// Locate the record within its original buffer, retaining byte offsets for readers.
    pub(crate) fn normalize(&mut self, line: &[u8]) -> Range<usize> {
        let mut record = trim_whitespace_range(line);
        if !self.record_seen && !self.bom_consumed && line[record.clone()].starts_with(BOM) {
            self.bom_consumed = true;
            let start = record.start + BOM.len();
            let content = trim_whitespace_range(&line[start..record.end]);
            record = start + content.start..start + content.end;
        }
        self.record_seen |= !record.is_empty();
        record
    }
}

/// Bounded projections skip only the preamble, leaving the header in the reader.
pub(crate) fn skip_preamble(reader: &mut impl BufRead) -> Result<u64, DomainError> {
    let mut offset = 0;
    let mut bom_consumed = false;
    loop {
        let buffer = reader.fill_buf().map_err(read_error)?;
        let whitespace = buffer
            .iter()
            .take_while(|&&byte| is_whitespace(byte))
            .count();
        if whitespace > 0 {
            reader.consume(whitespace);
            offset += whitespace as u64;
            continue;
        }
        if !bom_consumed && buffer.first() == Some(&BOM[0]) {
            let mut bytes = [0; 3];
            reader.read_exact(&mut bytes).map_err(|error| {
                if error.kind() == std::io::ErrorKind::UnexpectedEof {
                    DomainError::InvalidData("Incomplete UTF-8 in chat header preamble".into())
                } else {
                    read_error(error)
                }
            })?;
            if bytes != BOM {
                return Err(DomainError::InvalidData(
                    "Invalid chat header preamble".into(),
                ));
            }
            bom_consumed = true;
            offset += 3;
            continue;
        }
        return Ok(offset);
    }
}

fn validate_integrity(value: &Value) -> Result<&str, &'static str> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or("Chat metadata integrity must be a non-empty string")
}

pub(crate) fn validate_metadata_integrity(metadata: &Value) -> Result<Option<&str>, DomainError> {
    metadata
        .get("integrity")
        .map(validate_integrity)
        .transpose()
        // Format errors must not trigger the identity-conflict force-save dialog.
        .map_err(|message| DomainError::InvalidData(message.into()))
}

pub(crate) fn parse_header(bytes: &[u8]) -> Result<Map<String, Value>, DomainError> {
    let header: Map<String, Value> = serde_json::from_slice(bytes).map_err(|error| {
        DomainError::InvalidData(format!("Invalid chat header object: {error}"))
    })?;
    if let Some(metadata) = header.get("chat_metadata") {
        validate_metadata_integrity(metadata)?;
    }
    Ok(header)
}

/// Read identity without materializing the header's unknown metadata or extension values.
pub(crate) fn parse_header_integrity(bytes: &[u8]) -> Result<Option<String>, DomainError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        DomainError::InvalidData(format!("Chat header is not valid UTF-8: {error}"))
    })?;
    let fields: HashMap<String, &RawValue> = serde_json::from_str(text).map_err(|error| {
        DomainError::InvalidData(format!("Invalid chat header object: {error}"))
    })?;
    if let Some(metadata) = fields.get("chat_metadata") {
        return parse_metadata_integrity(metadata);
    }

    Ok(None)
}

pub(crate) fn parse_metadata_integrity(metadata: &RawValue) -> Result<Option<String>, DomainError> {
    if !metadata.get().starts_with('{') {
        return Ok(None);
    }
    let metadata: HeaderMetadata = serde_json::from_str(metadata.get())
        .map_err(|error| DomainError::InvalidData(format!("Invalid chat metadata: {error}")))?;
    Ok(metadata.integrity)
}

pub(crate) fn parse_record(bytes: &[u8]) -> Result<Value, DomainError> {
    serde_json::from_slice::<Map<String, Value>>(bytes)
        .map(Value::Object)
        .map_err(|error| DomainError::InvalidData(format!("Invalid chat record object: {error}")))
}

fn read_error(error: std::io::Error) -> DomainError {
    DomainError::InternalError(format!("Failed to read chat header: {error}"))
}

/// Locate the first record; the caller parses it into its required representation once.
/// The returned offset includes skipped preamble and the header's terminating newline.
/// The same reader remains positioned at the body, including any buffered bytes.
pub(crate) fn read_header_record(
    reader: &mut impl BufRead,
) -> Result<Option<(Vec<u8>, u64)>, DomainError> {
    let mut prefix = RecordPrefix::default();
    let mut bytes = Vec::new();
    let mut offset = 0;
    loop {
        bytes.clear();
        let len = reader.read_until(b'\n', &mut bytes).map_err(read_error)?;
        if len == 0 {
            return Ok(None);
        }
        offset += len as u64;
        let record = prefix.normalize(&bytes);
        if !record.is_empty() {
            bytes.truncate(record.end);
            bytes.drain(..record.start);
            return Ok(Some((bytes, offset)));
        }
    }
}

pub(crate) async fn read_header_record_async(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> Result<Option<(Vec<u8>, u64)>, DomainError> {
    let mut prefix = RecordPrefix::default();
    let mut bytes = Vec::new();
    let mut offset = 0;
    loop {
        bytes.clear();
        let len = reader
            .read_until(b'\n', &mut bytes)
            .await
            .map_err(read_error)?;
        if len == 0 {
            return Ok(None);
        }
        offset += len as u64;
        let record = prefix.normalize(&bytes);
        if !record.is_empty() {
            bytes.truncate(record.end);
            bytes.drain(..record.start);
            return Ok(Some((bytes, offset)));
        }
    }
}

/// Read a complete chat payload. A malformed record rejects the entire read.
pub(crate) async fn read_payload(path: &Path) -> Result<Vec<Value>, DomainError> {
    let file = File::open(path).await.map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            DomainError::NotFound(format!("Chat not found: {}", path.display()))
        }
        _ => DomainError::InternalError(format!("Failed to open chat {}: {error}", path.display())),
    })?;

    let mut reader = BufReader::new(file);
    let mut prefix = RecordPrefix::default();
    let mut objects = Vec::new();
    let mut line = Vec::new();
    let mut line_number = 0;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read JSONL file {}: {error}",
                path.display()
            ))
        })? == 0
        {
            return Ok(objects);
        }
        line_number += 1;
        let record = prefix.normalize(&line);
        if record.is_empty() {
            continue;
        }
        let record = &line[record];
        let object = if objects.is_empty() {
            parse_header(record).map(Value::Object)
        } else {
            parse_record(record)
        }
        .map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid chat record at line {line_number} in {}: {error}",
                path.display()
            ))
        })?;
        objects.push(object);
    }
}

/// Serialize a complete payload; callers own the submission and identity policy.
pub(crate) async fn write_payload(path: &Path, objects: &[Value]) -> Result<(), DomainError> {
    let mut bytes = Vec::new();
    for object in objects {
        serde_json::to_writer(&mut bytes, object).map_err(|error| {
            DomainError::InternalError(format!("Failed to serialize chat record: {error}"))
        })?;
        bytes.push(b'\n');
    }
    write_payload_bytes(path, &bytes).await
}

/// Publish bytes unchanged, using the shared file sync and atomic replacement mechanism.
pub(crate) async fn write_payload_bytes(path: &Path, bytes: &[u8]) -> Result<(), DomainError> {
    let temp_path = unique_temp_path(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat stage {}: {error}",
                temp_path.display()
            ))
        })?;
    file.write_all(bytes).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write chat stage {}: {error}",
            temp_path.display()
        ))
    })?;
    file.flush().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to flush chat stage {}: {error}",
            temp_path.display()
        ))
    })?;
    persist_file(file, &temp_path, path).await
}
