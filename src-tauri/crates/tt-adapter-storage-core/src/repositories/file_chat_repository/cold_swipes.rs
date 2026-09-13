use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_payload_commit_repository::{
    ChatSwipeSource, RestoredChatPayload,
};
use tt_ports::repositories::chat_repository::ChatByteReader;

const COLD: &str = "tt_swipe_cold";
type Fields<'a> = IndexMap<String, &'a RawValue>;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ColdReference {
    source_id: u32,
    record: usize,
}

#[derive(Clone, Copy)]
struct RecordSpan {
    start: u64,
    end: u64,
}

pub(super) struct FileSwipeSource {
    file: Mutex<File>,
    size: u64,
    modified: SystemTime,
    spans: Mutex<Vec<RecordSpan>>,
}

impl FileSwipeSource {
    pub(super) async fn open(path: &Path) -> Result<Arc<dyn ChatSwipeSource>, DomainError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = File::open(&path).map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => DomainError::NotFound(path.display().to_string()),
                _ => failure(format!("open {}: {error}", path.display())),
            })?;
            let metadata = file.metadata().map_err(failure)?;
            Ok(Arc::new(Self {
                size: metadata.len(),
                modified: metadata.modified().map_err(failure)?,
                file: Mutex::new(file),
                spans: Mutex::new(Vec::new()),
            }) as Arc<dyn ChatSwipeSource>)
        })
        .await
        .map_err(failure)?
    }

    fn check_file(&self) -> io::Result<()> {
        let metadata = self.file.lock().unwrap().metadata()?;
        if metadata.len() != self.size || metadata.modified()? != self.modified {
            return Err(io::Error::other(
                "Cold swipe source was modified; reopen the chat",
            ));
        }
        Ok(())
    }

    fn range(self: &Arc<Self>, span: RecordSpan) -> SourceRange {
        SourceRange {
            source: self.clone(),
            position: span.start,
            end: span.end,
        }
    }

    fn record_span(&self, record: usize) -> io::Result<RecordSpan> {
        self.spans
            .lock()
            .unwrap()
            .get(record)
            .copied()
            .filter(|_| record != 0)
            .ok_or_else(|| {
                io::Error::other(format!("Cold swipe source record {record} is unavailable"))
            })
    }
}

/// Every range has its own logical position. The shared file cursor is used only under the lock.
struct SourceRange {
    source: Arc<FileSwipeSource>,
    position: u64,
    end: u64,
}

impl Read for SourceRange {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let len = buffer.len().min((self.end - self.position) as usize);
        if len == 0 {
            return Ok(0);
        }
        let mut file = self.source.file.lock().unwrap();
        file.seek(SeekFrom::Start(self.position))?;
        let n = file.read(&mut buffer[..len])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Cold swipe source ended early",
            ));
        }
        self.position += n as u64;
        Ok(n)
    }
}

struct BlockingReader<R>(Arc<Mutex<R>>);

#[async_trait]
impl<R: Read + Send + 'static> ChatByteReader for BlockingReader<R> {
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
        .map_err(failure)?
        .map_err(failure)?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

fn byte_reader(reader: impl Read + Send + 'static) -> Box<dyn ChatByteReader> {
    Box::new(BlockingReader(Arc::new(Mutex::new(reader))))
}

/// Reads nonempty JSONL records while keeping offsets in the original bytes, including skipped lines.
struct Records<R> {
    reader: BufReader<R>,
    offset: u64,
    first: bool,
}

impl<R: Read> Records<R> {
    fn new(reader: R) -> Self {
        Self {
            reader: BufReader::with_capacity(64 * 1024, reader),
            offset: 0,
            first: true,
        }
    }

    fn next(&mut self, buffer: &mut Vec<u8>) -> io::Result<Option<RecordSpan>> {
        loop {
            buffer.clear();
            let start = self.offset;
            let n = self.reader.read_until(b'\n', buffer)?;
            if n == 0 {
                return Ok(None);
            }
            self.offset += n as u64;
            let mut begin = buffer.iter().position(|b| *b > 32).unwrap_or(n);
            let end = buffer
                .iter()
                .rposition(|b| *b > 32)
                .map_or(begin, |i| i + 1);
            if self.first && buffer[begin..end].starts_with(b"\xef\xbb\xbf") {
                begin += 3;
            }
            if begin == end {
                continue;
            }
            self.first = false;
            buffer.drain(..begin);
            buffer.truncate(end - begin);
            return Ok(Some(RecordSpan {
                start: start + begin as u64,
                end: start + end as u64,
            }));
        }
    }
}

struct ProjectionReader {
    source: Arc<FileSwipeSource>,
    source_id: u32,
    records: Records<SourceRange>,
    pending: Vec<u8>,
    next: Vec<u8>,
    output: Cursor<Vec<u8>>,
    record: usize,
    finished: bool,
}

impl ProjectionReader {
    fn fill_output(&mut self) -> io::Result<bool> {
        if self.finished {
            return Ok(false);
        }
        self.output.set_position(0);
        self.output.get_mut().clear();
        if self.record == 0 {
            let Some(span) = self.records.next(&mut self.pending)? else {
                self.finished = true;
                return Ok(false);
            };
            self.source.spans.lock().unwrap().push(span);
            // Validate without materializing the header's unknown JSON values.
            serde_json::from_slice::<Fields<'_>>(&self.pending)?;
            std::mem::swap(self.output.get_mut(), &mut self.pending);
            self.pending.clear();
        } else {
            if self.pending.is_empty() {
                let Some(span) = self.records.next(&mut self.pending)? else {
                    self.finished = true;
                    return Ok(false);
                };
                self.source.spans.lock().unwrap().push(span);
            }
            let next_span = self.records.next(&mut self.next)?;
            if let Some(span) = next_span {
                self.source.spans.lock().unwrap().push(span);
                project(
                    &self.pending,
                    self.source_id,
                    self.record,
                    self.output.get_mut(),
                )?;
            } else {
                serde_json::from_slice::<Fields<'_>>(&self.pending)?;
                std::mem::swap(self.output.get_mut(), &mut self.pending);
                self.finished = true;
                // The lookahead buffer may have held a much larger historical record.
                self.next = Vec::new();
            }
            std::mem::swap(&mut self.pending, &mut self.next);
        }
        self.record += 1;
        self.output.get_mut().push(b'\n');
        Ok(true)
    }
}

impl Read for ProjectionReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < buffer.len() {
            let n = self.output.read(&mut buffer[written..])?;
            written += n;
            if n == 0 && !self.fill_output()? {
                break;
            }
        }
        Ok(written)
    }
}

fn array<'a>(fields: &Fields<'a>, key: &str) -> Option<Vec<&'a RawValue>> {
    serde_json::from_str(fields.get(key)?.get()).ok()
}

fn swipe_arrays<'a>(fields: &Fields<'a>) -> Option<(usize, Vec<&'a RawValue>, Vec<&'a RawValue>)> {
    let index = serde_json::from_str::<usize>(fields.get("swipe_id")?.get()).ok()?;
    let swipes = array(fields, "swipes")?;
    let info = array(fields, "swipe_info")?;
    (swipes.len() > 1 && index < swipes.len() && info.len() == swipes.len())
        .then_some((index, swipes, info))
}

fn write_message(
    output: &mut impl Write,
    fields: &Fields<'_>,
    swipes: &[Option<&RawValue>],
    info: &[Option<&RawValue>],
    cold: Option<&ColdReference>,
) -> io::Result<()> {
    let mut serializer = serde_json::Serializer::new(output);
    let mut map = serde::Serializer::serialize_map(&mut serializer, None)?;
    for (key, value) in fields {
        match key.as_str() {
            "swipes" => map.serialize_entry(key, swipes)?,
            "swipe_info" => map.serialize_entry(key, info)?,
            COLD => {}
            _ => map.serialize_entry(key, value)?,
        }
    }
    if let Some(cold) = cold {
        map.serialize_entry(COLD, cold)?;
    }
    map.end()?;
    Ok(())
}

fn project(input: &[u8], source_id: u32, record: usize, output: &mut Vec<u8>) -> io::Result<()> {
    let fields: Fields<'_> = serde_json::from_slice(input)?;
    if let Some((index, swipes, info)) = swipe_arrays(&fields)
        && swipes.iter().all(|v| v.get().starts_with('"'))
        && info.iter().all(|v| v.get().starts_with('{'))
    {
        let mut cold_swipes = vec![None; swipes.len()];
        let mut cold_info = vec![None; info.len()];
        cold_swipes[index] = Some(swipes[index]);
        cold_info[index] = Some(info[index]);
        write_message(
            output,
            &fields,
            &cold_swipes,
            &cold_info,
            Some(&ColdReference { source_id, record }),
        )?;
    } else {
        output.extend_from_slice(input);
    }
    Ok(())
}

fn restore(input: &[u8], original: &[u8], output: &mut impl Write) -> io::Result<()> {
    let fields: Fields<'_> = serde_json::from_slice(input)?;
    let original: Fields<'_> = serde_json::from_slice(original)?;
    let invalid =
        || io::Error::other("Cold swipe structure changed; load swipes before modifying history");
    let (_, mut live_swipes, mut live_info) = swipe_arrays(&fields).ok_or_else(invalid)?;
    let (_, swipes, info) = swipe_arrays(&original).ok_or_else(invalid)?;
    if live_swipes.len() < swipes.len() {
        return Err(invalid());
    }
    // Null is unloaded; every live value (including appended slots) is authoritative.
    for (live, original) in live_swipes
        .iter_mut()
        .zip(swipes)
        .chain(live_info.iter_mut().zip(info))
    {
        if live.get() == "null" {
            *live = original;
        }
    }
    let swipes: Vec<_> = live_swipes.into_iter().map(Some).collect();
    let info: Vec<_> = live_info.into_iter().map(Some).collect();
    write_message(output, &fields, &swipes, &info, None)
}

struct Output {
    file: BufWriter<File>,
    size: u64,
    hasher: Option<Sha256>,
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = self.file.write(bytes)?;
        self.size += n as u64;
        if let Some(hasher) = &mut self.hasher {
            hasher.update(&bytes[..n]);
        }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[async_trait]
impl ChatSwipeSource for FileSwipeSource {
    fn projection(self: Arc<Self>, source_id: u32) -> Box<dyn ChatByteReader> {
        let records = Records::new(self.range(RecordSpan {
            start: 0,
            end: self.size,
        }));
        byte_reader(ProjectionReader {
            source: self,
            source_id,
            records,
            pending: Vec::new(),
            next: Vec::new(),
            output: Cursor::new(Vec::new()),
            record: 0,
            finished: false,
        })
    }

    async fn record(
        self: Arc<Self>,
        record: usize,
    ) -> Result<Box<dyn ChatByteReader>, DomainError> {
        tokio::task::spawn_blocking(move || {
            self.check_file().map_err(failure)?;
            let span = self.record_span(record).map_err(failure)?;
            Ok(byte_reader(self.range(span)))
        })
        .await
        .map_err(failure)?
    }

    async fn restore_payload(
        self: Arc<Self>,
        source_id: u32,
        input: &Path,
        output: &Path,
        hash: bool,
    ) -> Result<RestoredChatPayload, DomainError> {
        let input = input.to_owned();
        let output = output.to_owned();
        tokio::task::spawn_blocking(move || {
            self.check_file()?;
            let mut records = Records::new(File::open(input)?);
            let mut output = Output {
                file: BufWriter::new(
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(output)?,
                ),
                size: 0,
                hasher: hash.then(Sha256::new),
            };
            let mut line = Vec::new();
            let mut original = Vec::new();
            while records.next(&mut line)?.is_some() {
                let fields: Fields<'_> = serde_json::from_slice(&line)?;
                if let Some(reference) = fields.get(COLD) {
                    let reference: ColdReference = serde_json::from_str(reference.get())?;
                    if reference.source_id != source_id {
                        return Err(io::Error::other("Chat commit mixes cold swipe sources"));
                    }
                    original.clear();
                    self.range(self.record_span(reference.record)?)
                        .read_to_end(&mut original)?;
                    restore(&line, &original, &mut output)?;
                } else {
                    output.write_all(&line)?;
                }
                output.write_all(b"\n")?;
            }
            output.flush()?;
            Ok::<_, io::Error>(RestoredChatPayload {
                size: output.size,
                sha256: output.hasher.map(|v| v.finalize().into()),
            })
        })
        .await
        .map_err(failure)?
        .map_err(failure)
    }
}

fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Cold swipes: {error}"))
}
