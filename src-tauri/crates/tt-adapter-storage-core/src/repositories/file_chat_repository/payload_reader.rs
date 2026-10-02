use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use serde_json::value::RawValue;
use tt_domain::errors::DomainError;
use tt_ports::byte_reader::ByteReader;

use crate::chat_jsonl::{RecordPrefix, parse_header_integrity};

struct BlockingReader<R>(Arc<Mutex<R>>);

#[async_trait]
impl<R: Read + Send + 'static> ByteReader for BlockingReader<R> {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, DomainError> {
        let reader = self.0.clone();
        let len = buffer.len();
        let bytes = tokio::task::spawn_blocking(move || {
            let mut bytes = vec![0; len];
            let n = reader.lock().unwrap().read(&mut bytes)?;
            bytes.truncate(n);
            Ok::<_, io::Error>(bytes)
        })
        .await
        .map_err(read_error)?
        .map_err(io_error)?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

pub(super) fn blocking_reader(reader: impl Read + Send + 'static) -> Box<dyn ByteReader> {
    Box::new(BlockingReader(Arc::new(Mutex::new(reader))))
}

/// The opened file is the source of truth even after its path is replaced.
pub(super) struct OpenedChatFile {
    file: Mutex<File>,
    size: u64,
    modified: SystemTime,
}

impl OpenedChatFile {
    pub(super) async fn open(path: &Path) -> Result<Arc<Self>, DomainError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = File::open(&path).map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => DomainError::NotFound(path.display().to_string()),
                _ => DomainError::InternalError(format!(
                    "Failed to open {}: {error}",
                    path.display()
                )),
            })?;
            let metadata = file.metadata().map_err(io_error)?;
            Ok(Arc::new(Self {
                size: metadata.len(),
                modified: metadata.modified().map_err(io_error)?,
                file: Mutex::new(file),
            }))
        })
        .await
        .map_err(read_error)?
    }

    pub(super) fn check_unchanged(&self) -> io::Result<()> {
        let metadata = self.file.lock().unwrap().metadata()?;
        if metadata.len() != self.size || metadata.modified()? != self.modified {
            return Err(io::Error::other(
                "Chat source was modified; reopen the chat",
            ));
        }
        Ok(())
    }

    pub(super) fn full_range(self: &Arc<Self>) -> SourceRange {
        self.range(RecordSpan {
            start: 0,
            end: self.size,
        })
    }

    pub(super) fn range(self: &Arc<Self>, span: RecordSpan) -> SourceRange {
        SourceRange {
            source: self.clone(),
            position: span.start,
            end: span.end,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct RecordSpan {
    pub(super) start: u64,
    pub(super) end: u64,
}

/// Each reader advances its own range; only the physical file cursor is shared.
pub(super) struct SourceRange {
    source: Arc<OpenedChatFile>,
    position: u64,
    end: u64,
}

impl Read for SourceRange {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let len = (self.end - self.position).min(buffer.len() as u64) as usize;
        if len == 0 {
            return Ok(0);
        }
        let mut file = self.source.file.lock().unwrap();
        file.seek(SeekFrom::Start(self.position))?;
        let n = file.read(&mut buffer[..len])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Chat file ended before its declared size",
            ));
        }
        self.position += n as u64;
        Ok(n)
    }
}

/// Iterate normalized records while retaining their offsets in the original JSONL.
pub(super) struct Records<R> {
    reader: BufReader<R>,
    offset: u64,
    line: usize,
    prefix: RecordPrefix,
}

impl<R: Read> Records<R> {
    pub(super) fn new(reader: R) -> Self {
        Self {
            reader: BufReader::with_capacity(64 * 1024, reader),
            offset: 0,
            line: 0,
            prefix: RecordPrefix::default(),
        }
    }

    pub(super) fn next(&mut self, buffer: &mut Vec<u8>) -> io::Result<Option<RecordSpan>> {
        loop {
            buffer.clear();
            let start = self.offset;
            let n = self.reader.read_until(b'\n', buffer)?;
            if n == 0 {
                return Ok(None);
            }
            self.offset += n as u64;
            self.line += 1;
            let normalized = self.prefix.normalize(buffer);
            if normalized.is_empty() {
                continue;
            }
            buffer.truncate(normalized.end);
            buffer.drain(..normalized.start);
            return Ok(Some(RecordSpan {
                start: start + normalized.start as u64,
                end: start + normalized.end as u64,
            }));
        }
    }
}

pub(super) async fn open_json_array(path: &Path) -> Result<Box<dyn ByteReader>, DomainError> {
    let source = OpenedChatFile::open(path).await?;
    Ok(blocking_reader(JsonArrayReader {
        records: Records::new(source.full_range()),
        record: Cursor::new(Vec::new()),
        first: true,
        separator: Some(b'['),
        finished: false,
    }))
}

/// One opened source and one record buffer; JSON values are validated without materializing them.
struct JsonArrayReader<R> {
    records: Records<R>,
    record: Cursor<Vec<u8>>,
    first: bool,
    separator: Option<u8>,
    finished: bool,
}

impl<R: Read> JsonArrayReader<R> {
    fn next_record(&mut self) -> io::Result<bool> {
        self.record.set_position(0);
        if self.records.next(self.record.get_mut())?.is_none() {
            return Ok(false);
        }
        validate_record(self.record.get_ref(), self.first).map_err(|error| {
            invalid_data(format!(
                "Invalid JSONL at line {}: {error}",
                self.records.line
            ))
        })?;
        Ok(true)
    }
}

impl<R: Read> Read for JsonArrayReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < buffer.len() {
            if let Some(separator) = self.separator.take() {
                buffer[written] = separator;
                written += 1;
                continue;
            }
            let n = self.record.read(&mut buffer[written..])?;
            written += n;
            if n != 0 {
                continue;
            }
            if self.finished {
                break;
            }
            if self.next_record()? {
                self.separator = (!self.first).then_some(b',');
                self.first = false;
            } else {
                self.separator = Some(b']');
                self.finished = true;
            }
        }
        Ok(written)
    }
}

fn validate_record(bytes: &[u8], header: bool) -> io::Result<()> {
    if header {
        return parse_header_integrity(bytes)
            .map(|_| ())
            .map_err(invalid_data);
    }
    let text = std::str::from_utf8(bytes).map_err(invalid_data)?;
    let value: &RawValue = serde_json::from_str(text).map_err(invalid_data)?;
    if !value.get().starts_with('{') {
        return Err(invalid_data("Chat record must be an object"));
    }
    Ok(())
}

pub(super) fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

pub(super) fn io_error(error: io::Error) -> DomainError {
    match error.kind() {
        io::ErrorKind::InvalidData => DomainError::InvalidData(error.to_string()),
        _ => read_error(error),
    }
}

fn read_error(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Failed to read chat bytes: {error}"))
}
