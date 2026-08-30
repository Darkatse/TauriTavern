use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tt_domain::errors::DomainError;

use super::super::backup_codec::{BackupFormat, read_zstd_frame_content_size};

const BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Default, Deserialize)]
pub(super) struct HeaderProjection {
    pub(super) character_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present_json_value")]
    pub(super) chat_metadata: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct TailProjection {
    pub(super) mes: Option<String>,
    pub(super) send_date: Option<Value>,
}

pub(super) struct FileProjection {
    pub(super) line_count: usize,
    pub(super) header: HeaderProjection,
    pub(super) tail: TailProjection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RecordSpan {
    start: u64,
    end: u64,
}

impl RecordSpan {
    fn len(self) -> u64 {
        self.end - self.start
    }
}

#[derive(Default)]
struct SpanScan {
    line_count: usize,
    first: Option<RecordSpan>,
    last: Option<RecordSpan>,
}

fn deserialize_present_json_value<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

pub(super) async fn read_last_raw_record(
    file: &mut File,
    path: &Path,
    file_size: u64,
) -> Result<Option<TailProjection>, DomainError> {
    let Some(span) = find_last_record_span(file, path, file_size).await? else {
        return Ok(None);
    };
    let task_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || read_raw_record(&task_path, span))
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Chat stats projection task failed for {}: {error}",
                path.display()
            ))
        })?
        .map(Some)
}

pub(super) async fn scan_file(path: &Path) -> Result<FileProjection, DomainError> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            DomainError::InvalidData(format!("Invalid chat backup path: {}", path.display()))
        })?;
    let (format, _) = BackupFormat::parse_physical_file_name(file_name).ok_or_else(|| {
        DomainError::InvalidData(format!("Unsupported chat backup file name: {file_name}"))
    })?;
    if format == BackupFormat::Zstd {
        read_zstd_frame_content_size(path).await?;
    }

    let task_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || scan_file_blocking(&task_path, format))
        .await
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Chat summary projection task failed for {}: {error}",
                path.display()
            ))
        })?
}

async fn find_last_record_span(
    file: &mut File,
    path: &Path,
    file_size: u64,
) -> Result<Option<RecordSpan>, DomainError> {
    if file_size == 0 {
        return Ok(None);
    }

    let mut position = file_size;
    let mut record_end = file_size;
    let mut has_content = false;
    let mut buffer = vec![0; BUFFER_BYTES];

    while position > 0 {
        let read_len = position.min(BUFFER_BYTES as u64) as usize;
        position -= read_len as u64;
        file.seek(SeekFrom::Start(position))
            .await
            .map_err(|error| file_io_error("seek", path, error))?;
        file.read_exact(&mut buffer[..read_len])
            .await
            .map_err(|error| file_io_error("read", path, error))?;

        for (index, &byte) in buffer[..read_len].iter().enumerate().rev() {
            let offset = position + index as u64;
            if byte == b'\n' {
                if has_content {
                    return Ok(Some(RecordSpan {
                        start: offset + 1,
                        end: record_end,
                    }));
                }
                record_end = offset;
                has_content = false;
            } else if !byte.is_ascii_whitespace() {
                has_content = true;
            }
        }
    }

    Ok(has_content.then_some(RecordSpan {
        start: 0,
        end: record_end,
    }))
}

fn scan_file_blocking(path: &Path, format: BackupFormat) -> Result<FileProjection, DomainError> {
    // Raw files can seek; zstd needs one bounded pass for decoded spans, then
    // another decoder that parses only the header and tail projections.
    let spans = match format {
        BackupFormat::RawJsonl => scan_record_spans(open_raw(path)?, path, format)?,
        BackupFormat::Zstd => scan_record_spans(open_zstd(path)?, path, format)?,
    };
    let Some(first_span) = spans.first else {
        return Ok(FileProjection {
            line_count: 0,
            header: HeaderProjection::default(),
            tail: TailProjection::default(),
        });
    };
    let last_span = spans.last.unwrap_or(first_span);

    let (header, tail) = match format {
        BackupFormat::RawJsonl => (
            read_raw_record(path, first_span)?,
            read_raw_record(path, last_span)?,
        ),
        BackupFormat::Zstd => read_zstd_records(path, first_span, last_span)?,
    };

    Ok(FileProjection {
        line_count: spans.line_count,
        header,
        tail,
    })
}

fn open_raw(path: &Path) -> Result<std::fs::File, DomainError> {
    std::fs::File::open(path).map_err(|error| file_io_error("open", path, error))
}

fn open_zstd(path: &Path) -> Result<impl Read, DomainError> {
    zstd::stream::read::Decoder::new(open_raw(path)?).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to decode Zstandard chat backup {}: {error}",
            path.display()
        ))
    })
}

fn scan_record_spans(
    mut reader: impl Read,
    path: &Path,
    format: BackupFormat,
) -> Result<SpanScan, DomainError> {
    let mut scan = SpanScan::default();
    let mut buffer = vec![0; BUFFER_BYTES];
    let mut utf8_pending = Vec::with_capacity(3);
    let mut utf8_scratch = Vec::with_capacity(BUFFER_BYTES + 3);
    let mut offset = 0u64;
    let mut record_start = 0u64;
    let mut has_content = false;

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|error| read_error(path, format, error))?;
        if bytes_read == 0 {
            break;
        }
        validate_utf8_chunk(
            path,
            offset,
            &buffer[..bytes_read],
            &mut utf8_pending,
            &mut utf8_scratch,
        )?;

        for &byte in &buffer[..bytes_read] {
            if byte == b'\n' {
                record_finished(&mut scan, record_start, offset, has_content);
                record_start = offset + 1;
                has_content = false;
            } else if !byte.is_ascii_whitespace() {
                has_content = true;
            }
            offset += 1;
        }
    }

    if !utf8_pending.is_empty() {
        return Err(DomainError::InvalidData(format!(
            "Chat file {} ends with incomplete UTF-8 at byte {}",
            path.display(),
            offset - utf8_pending.len() as u64
        )));
    }
    record_finished(&mut scan, record_start, offset, has_content);
    Ok(scan)
}

fn record_finished(scan: &mut SpanScan, start: u64, end: u64, has_content: bool) {
    if !has_content {
        return;
    }
    let span = RecordSpan { start, end };
    scan.line_count += 1;
    scan.first.get_or_insert(span);
    scan.last = Some(span);
}

fn validate_utf8_chunk(
    path: &Path,
    offset: u64,
    bytes: &[u8],
    pending: &mut Vec<u8>,
    scratch: &mut Vec<u8>,
) -> Result<(), DomainError> {
    let pending_len = pending.len();
    scratch.clear();
    scratch.extend_from_slice(pending);
    scratch.extend_from_slice(bytes);
    pending.clear();

    match std::str::from_utf8(scratch) {
        Ok(_) => Ok(()),
        Err(error) if error.error_len().is_none() => {
            pending.extend_from_slice(&scratch[error.valid_up_to()..]);
            Ok(())
        }
        Err(error) => Err(DomainError::InvalidData(format!(
            "Chat file {} contains invalid UTF-8 at byte {}",
            path.display(),
            offset - pending_len as u64 + error.valid_up_to() as u64
        ))),
    }
}

fn read_raw_record<T: DeserializeOwned>(path: &Path, span: RecordSpan) -> Result<T, DomainError> {
    let mut file = open_raw(path)?;
    file.seek(SeekFrom::Start(span.start))
        .map_err(|error| file_io_error("seek", path, error))?;
    parse_record(file.take(span.len()), path, span)
}

fn read_zstd_records(
    path: &Path,
    first_span: RecordSpan,
    last_span: RecordSpan,
) -> Result<(HeaderProjection, TailProjection), DomainError> {
    let mut decoded = open_zstd(path)?;
    skip_decoded(&mut decoded, first_span.start, path)?;
    let header = parse_record((&mut decoded).take(first_span.len()), path, first_span)?;
    if first_span == last_span {
        let mut decoded = open_zstd(path)?;
        skip_decoded(&mut decoded, last_span.start, path)?;
        let tail = parse_record((&mut decoded).take(last_span.len()), path, last_span)?;
        return Ok((header, tail));
    }

    skip_decoded(&mut decoded, last_span.start - first_span.end, path)?;
    let tail = parse_record((&mut decoded).take(last_span.len()), path, last_span)?;
    Ok((header, tail))
}

fn skip_decoded(reader: &mut impl Read, bytes: u64, path: &Path) -> Result<(), DomainError> {
    let copied = io::copy(&mut reader.take(bytes), &mut io::sink()).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to decode Zstandard chat backup {}: {error}",
            path.display()
        ))
    })?;
    if copied != bytes {
        return Err(DomainError::InvalidData(format!(
            "Zstandard chat backup {} ended at decoded byte {copied}, expected {bytes}",
            path.display()
        )));
    }
    Ok(())
}

fn parse_record<T: DeserializeOwned>(
    reader: impl Read,
    path: &Path,
    span: RecordSpan,
) -> Result<T, DomainError> {
    let mut deserializer =
        serde_json::Deserializer::from_reader(BufReader::with_capacity(BUFFER_BYTES, reader));
    let projection = T::deserialize(&mut deserializer).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to parse chat record {}..{} in {}: {error}",
            span.start,
            span.end,
            path.display()
        ))
    })?;
    deserializer.end().map_err(|error| {
        DomainError::InvalidData(format!(
            "Unexpected trailing data in chat record {}..{} in {}: {error}",
            span.start,
            span.end,
            path.display()
        ))
    })?;
    Ok(projection)
}

fn file_io_error(operation: &str, path: &Path, error: io::Error) -> DomainError {
    DomainError::InternalError(format!(
        "Failed to {operation} chat file {}: {error}",
        path.display()
    ))
}

fn read_error(path: &Path, format: BackupFormat, error: io::Error) -> DomainError {
    match format {
        BackupFormat::RawJsonl => file_io_error("read", path, error),
        BackupFormat::Zstd => DomainError::InvalidData(format!(
            "Failed to decode Zstandard chat backup {}: {error}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use rand::random;
    use serde_json::json;
    use tokio::fs;

    use super::*;

    #[tokio::test]
    async fn zstd_projection_is_bounded_and_never_accepts_raw_jsonl() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-zstd-summary-{}", random::<u64>()));
        fs::create_dir_all(&root)
            .await
            .expect("create test directory");
        let path = root.join("chat_alice_20260722-120000.jsonl.zst");
        let raw = [
            json!({"chat_metadata":{"chat_id_hash":42},"character_name":"Alice"}).to_string(),
            json!({"send_date":"2026-07-21T00:00:00.000Z","mes":"x".repeat(BUFFER_BYTES + 17)})
                .to_string(),
            json!({"send_date":"2026-07-22T00:00:00.000Z","mes":"tail response"}).to_string(),
        ]
        .join("\n");
        fs::write(&path, zstd::stream::encode_all(raw.as_bytes(), 1).unwrap())
            .await
            .unwrap();

        let projection = scan_file(&path).await.unwrap();
        assert_eq!(projection.line_count, 3);
        assert_eq!(projection.header.character_name.as_deref(), Some("Alice"));
        assert_eq!(projection.tail.mes.as_deref(), Some("tail response"));

        fs::write(&path, raw).await.unwrap();
        assert!(scan_file(&path).await.is_err());
        let _ = fs::remove_dir_all(root).await;
    }
}
