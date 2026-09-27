use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::chat_jsonl::{
    is_whitespace, skip_preamble, trim_whitespace, validate_metadata_integrity,
};
use serde::Deserialize;
use serde::de::{DeserializeOwned, IgnoredAny, MapAccess, Visitor};
use serde_json::Value;
use tt_domain::errors::DomainError;

use super::super::backup_codec::{BackupFormat, read_zstd_frame_content_size};

const BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Default, Deserialize)]
pub(super) struct HeaderProjection {
    pub(super) character_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_present_json_value")]
    pub(super) chat_metadata: Option<Value>,
}

#[derive(Debug, Default)]
pub(super) struct MessageProjection<Text> {
    pub(super) mes: Text,
    pub(super) send_date: Option<Value>,
}

pub(super) type TailProjection = MessageProjection<MessageText>;
type DateProjection = MessageProjection<IgnoredAny>;

impl<'de, Text: Default + Deserialize<'de>> Deserialize<'de> for MessageProjection<Text> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(Self::default())
    }
}

impl<'de, Text: Deserialize<'de>> Visitor<'de> for MessageProjection<Text> {
    type Value = Self;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a chat message object")
    }

    fn visit_map<M: MapAccess<'de>>(mut self, mut map: M) -> Result<Self, M::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Mes,
            SendDate,
            #[serde(other)]
            Other,
        }

        // Match JSON object loading: the last occurrence of each field wins.
        // Date-only reads consume `mes` as IgnoredAny, without allocating it.
        while let Some(field) = map.next_key()? {
            match field {
                Field::Mes => self.mes = map.next_value()?,
                Field::SendDate => self.send_date = map.next_value()?,
                Field::Other => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Default)]
pub(super) enum MessageText {
    #[default]
    Missing,
    Text(String),
    Unavailable,
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
    let value = Value::deserialize(deserializer)?;
    validate_metadata_integrity(&value).map_err(serde::de::Error::custom)?;
    Ok(Some(value))
}

impl<'de> Deserialize<'de> for MessageText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::Null => Self::Missing,
            Value::String(text) => Self::Text(text),
            _ => Self::Unavailable,
        })
    }
}

pub(super) async fn read_last_raw_date(path: &Path) -> Result<Option<Value>, DomainError> {
    let task_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let Some(reader) = open_last_raw_record(&task_path)? else {
            return Ok(None);
        };
        let reader = BufReader::with_capacity(BUFFER_BYTES, Utf8Reader::new(reader));
        read_message::<DateProjection>(reader, &task_path)
            .map(|record| record.and_then(|record| record.send_date))
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!(
            "Chat date projection failed for {}: {error}",
            path.display()
        ))
    })?
}

pub(super) async fn read_last_raw_tail(path: &Path) -> Result<TailProjection, DomainError> {
    let task_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let Some(mut reader) = open_last_raw_record(&task_path)? else {
            return Ok(TailProjection::default());
        };
        // Full-text callers already need an owned body. Buffer only this record
        // for the faster slice parser; unrelated fields still use IgnoredAny.
        let mut record = String::new();
        reader
            .read_to_string(&mut record)
            .map_err(|error| read_error(&task_path, BackupFormat::RawJsonl, error))?;
        parse_tail(record.as_bytes(), &task_path)
    })
    .await
    .map_err(|error| {
        DomainError::InternalError(format!(
            "Chat tail projection failed for {}: {error}",
            path.display()
        ))
    })?
}

// One handle owns the preamble, reverse lookup and selected record read.
fn open_last_raw_record(path: &Path) -> Result<Option<io::Take<File>>, DomainError> {
    let mut file = open_raw(path)?;
    let size = file
        .metadata()
        .map_err(|error| file_io_error("stat", path, error))?
        .len();
    let header_start = skip_preamble(&mut BufReader::with_capacity(BUFFER_BYTES, &mut file))?;
    if header_start == size {
        return Ok(None);
    }
    let Some(span) = find_last_record_span(&mut file, path, size)? else {
        return Ok(None);
    };
    let header_only = span.start <= header_start;
    let start = span.start.max(header_start);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| file_io_error("seek", path, error))?;
    let reader = file.take(span.end - start);
    if header_only {
        read_header(
            BufReader::with_capacity(BUFFER_BYTES, Utf8Reader::new(reader)),
            path,
        )?;
        Ok(None)
    } else {
        Ok(Some(reader))
    }
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

fn find_last_record_span(
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
            .map_err(|error| file_io_error("seek", path, error))?;
        file.read_exact(&mut buffer[..read_len])
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
            } else if !is_whitespace(byte) {
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
    let mut raw = open_raw(path)?;
    let spans = match format {
        BackupFormat::RawJsonl => scan_record_spans(&mut raw, path, format)?,
        BackupFormat::Zstd => scan_record_spans(open_zstd(&mut raw, path)?, path, format)?,
    };
    let Some(first_span) = spans.first else {
        return Ok(FileProjection {
            line_count: 0,
            header: HeaderProjection::default(),
            tail: TailProjection::default(),
        });
    };
    let last_span = spans.last.expect("a nonempty scan has a last record");
    let (header, tail) = match format {
        BackupFormat::RawJsonl => {
            let header = read_header(raw_record(&mut raw, path, first_span)?, path)?;
            let tail = if first_span == last_span {
                TailProjection::default()
            } else {
                read_tail(raw_record(&mut raw, path, last_span)?, path)?
            };
            (header, tail)
        }
        BackupFormat::Zstd => read_zstd_records(&mut raw, path, first_span, last_span)?,
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

fn open_zstd(file: &mut File, path: &Path) -> Result<impl Read, DomainError> {
    zstd::stream::read::Decoder::new(file).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to decode Zstandard chat backup {}: {error}",
            path.display()
        ))
    })
}

fn scan_record_spans(
    reader: impl Read,
    path: &Path,
    format: BackupFormat,
) -> Result<SpanScan, DomainError> {
    let mut scan = SpanScan::default();
    let mut reader = BufReader::with_capacity(BUFFER_BYTES, reader);
    let preamble_len = skip_preamble(&mut reader)?;
    let mut reader = Utf8Reader::new(reader);
    let mut buffer = vec![0; BUFFER_BYTES];
    let mut offset = preamble_len;
    let mut record_start = preamble_len;
    let mut has_content = false;

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|error| read_error(path, format, error))?;
        if bytes_read == 0 {
            break;
        }
        for &byte in &buffer[..bytes_read] {
            if byte == b'\n' {
                record_finished(&mut scan, record_start, offset, has_content);
                record_start = offset + 1;
                has_content = false;
            } else if !is_whitespace(byte) {
                has_content = true;
            }
            offset += 1;
        }
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

fn raw_record<'a>(
    file: &'a mut File,
    path: &Path,
    span: RecordSpan,
) -> Result<impl BufRead + 'a, DomainError> {
    file.seek(SeekFrom::Start(span.start))
        .map_err(|error| file_io_error("seek", path, error))?;
    Ok(BufReader::with_capacity(
        BUFFER_BYTES,
        file.take(span.len()),
    ))
}

fn read_zstd_records(
    file: &mut File,
    path: &Path,
    first_span: RecordSpan,
    last_span: RecordSpan,
) -> Result<(HeaderProjection, TailProjection), DomainError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| file_io_error("seek", path, error))?;
    let mut decoded = open_zstd(file, path)?;
    skip_decoded(&mut decoded, first_span.start, path)?;
    let header = read_header(
        BufReader::with_capacity(BUFFER_BYTES, (&mut decoded).take(first_span.len())),
        path,
    )?;
    if first_span == last_span {
        return Ok((header, TailProjection::default()));
    }

    skip_decoded(&mut decoded, last_span.start - first_span.end, path)?;
    let tail = read_tail(
        BufReader::with_capacity(BUFFER_BYTES, (&mut decoded).take(last_span.len())),
        path,
    )?;
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

// The line scanner has already validated UTF-8. Keep its slice parser separate
// from file readers so large records do not gain another streaming buffer.
pub(super) fn parse_header(bytes: &[u8], path: &Path) -> Result<HeaderProjection, DomainError> {
    if !trim_whitespace(bytes).starts_with(b"{") {
        return Err(DomainError::InvalidData(format!(
            "Chat header in {} must be an object",
            path.display()
        )));
    }
    serde_json::from_slice(bytes).map_err(|error| json_error(path, error))
}

pub(super) fn parse_tail(bytes: &[u8], path: &Path) -> Result<TailProjection, DomainError> {
    let record = recover_message(serde_json::from_slice(bytes), path)?;
    Ok(tail_projection(record, path))
}

fn read_header(mut reader: impl BufRead, path: &Path) -> Result<HeaderProjection, DomainError> {
    if !starts_object(&mut reader, path)? {
        return Err(DomainError::InvalidData(format!(
            "Chat header in {} must be an object",
            path.display()
        )));
    }
    serde_json::from_reader(reader).map_err(|error| json_error(path, error))
}

fn read_tail(reader: impl BufRead, path: &Path) -> Result<TailProjection, DomainError> {
    Ok(tail_projection(read_message(reader, path)?, path))
}

fn read_message<T: DeserializeOwned>(
    mut reader: impl BufRead,
    path: &Path,
) -> Result<Option<T>, DomainError> {
    let record = recover_message(serde_json::from_reader(&mut reader), path)?;
    if record.is_none() {
        // A local tail read must still validate its remaining selected bytes.
        io::copy(&mut reader, &mut io::sink())
            .map_err(|error| read_error(path, BackupFormat::RawJsonl, error))?;
    }
    Ok(record)
}

fn starts_object(reader: &mut impl BufRead, path: &Path) -> Result<bool, DomainError> {
    loop {
        let bytes = reader
            .fill_buf()
            .map_err(|error| read_error(path, BackupFormat::RawJsonl, error))?;
        let whitespace = bytes
            .iter()
            .take_while(|&&byte| is_whitespace(byte))
            .count();
        if whitespace == 0 {
            return Ok(bytes.first() == Some(&b'{'));
        }
        reader.consume(whitespace);
    }
}

fn recover_message<T>(
    result: serde_json::Result<T>,
    path: &Path,
) -> Result<Option<T>, DomainError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if !error.is_io() => {
            tracing::warn!(path = %path.display(), %error, "Last chat record is unavailable");
            Ok(None)
        }
        Err(error) => Err(json_error(path, error)),
    }
}

fn tail_projection(record: Option<TailProjection>, path: &Path) -> TailProjection {
    let Some(tail) = record else {
        return TailProjection {
            mes: MessageText::Unavailable,
            send_date: None,
        };
    };
    if matches!(tail.mes, MessageText::Unavailable) {
        tracing::warn!(path = %path.display(), "Chat preview unavailable: message text must be a string or null");
    }
    tail
}

fn json_error(path: &Path, error: serde_json::Error) -> DomainError {
    if error.is_io() {
        read_error(path, BackupFormat::RawJsonl, error.into())
    } else {
        DomainError::InvalidData(format!(
            "Invalid chat record in {}: {error}",
            path.display()
        ))
    }
}

// JSON field projection can skip string values without checking their encoding.
// Validate once while scanning a full file, or while reading a selected tail.
struct Utf8Reader<R> {
    inner: R,
    pending: Vec<u8>,
    scratch: Vec<u8>,
}

impl<R> Utf8Reader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            pending: Vec::with_capacity(3),
            scratch: Vec::with_capacity(BUFFER_BYTES + 3),
        }
    }
}

impl<R: Read> Read for Utf8Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let len = self.inner.read(buffer)?;
        if len == 0 && !self.pending.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Incomplete UTF-8 in chat record",
            ));
        }
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.pending);
        self.scratch.extend_from_slice(&buffer[..len]);
        self.pending.clear();
        match std::str::from_utf8(&self.scratch) {
            Ok(_) => {}
            Err(error) if error.error_len().is_none() => {
                self.pending
                    .extend_from_slice(&self.scratch[error.valid_up_to()..]);
            }
            Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
        }
        Ok(len)
    }
}

fn file_io_error(operation: &str, path: &Path, error: io::Error) -> DomainError {
    DomainError::InternalError(format!(
        "Failed to {operation} chat file {}: {error}",
        path.display()
    ))
}

fn read_error(path: &Path, format: BackupFormat, error: io::Error) -> DomainError {
    if error.kind() == io::ErrorKind::InvalidData {
        return DomainError::InvalidData(format!("Invalid chat file {}: {error}", path.display()));
    }
    match format {
        BackupFormat::RawJsonl => file_io_error("read", path, error),
        BackupFormat::Zstd => DomainError::InvalidData(format!(
            "Failed to decode Zstandard chat backup {}: {error}",
            path.display()
        )),
    }
}
