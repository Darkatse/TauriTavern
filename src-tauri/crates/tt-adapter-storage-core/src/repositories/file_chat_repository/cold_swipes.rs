use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Cursor, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::payload_reader::{
    OpenedChatFile, RecordSpan, Records, SourceRange, blocking_reader, invalid_data, io_error,
};
use crate::chat_jsonl::parse_header_integrity;
use async_trait::async_trait;
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use tt_domain::errors::DomainError;
use tt_ports::byte_reader::ByteReader;
use tt_ports::repositories::chat_commit_repository::{ChatSwipeSource, RestoredChatPayload};

const COLD: &str = "tt_swipe_cold";
type Fields<'a> = IndexMap<String, &'a RawValue>;

fn parse_fields(input: &[u8]) -> io::Result<Fields<'_>> {
    let text = std::str::from_utf8(input).map_err(invalid_data)?;
    serde_json::from_str(text).map_err(invalid_data)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ColdReference {
    source_id: u32,
    record: usize,
}

pub(super) struct FileSwipeSource {
    file: Arc<OpenedChatFile>,
    spans: Mutex<Vec<RecordSpan>>,
}

impl FileSwipeSource {
    pub(super) async fn open(path: &Path) -> Result<Arc<dyn ChatSwipeSource>, DomainError> {
        Ok(Arc::new(Self {
            file: OpenedChatFile::open(path).await?,
            spans: Mutex::new(Vec::new()),
        }))
    }

    fn record_span(&self, record: usize) -> io::Result<RecordSpan> {
        self.spans
            .lock()
            .unwrap()
            .get(record)
            .copied()
            .filter(|_| record != 0)
            .ok_or_else(|| {
                invalid_data(format!("Cold swipe source record {record} is unavailable"))
            })
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
            parse_header_integrity(&self.pending).map_err(invalid_data)?;
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
                parse_fields(&self.pending)?;
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
    let fields = parse_fields(input)?;
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

fn restore(fields: &Fields<'_>, original: &[u8], output: &mut impl Write) -> io::Result<()> {
    let original = parse_fields(original)?;
    let invalid =
        || invalid_data("Cold swipe structure changed; load swipes before modifying history");
    let (_, mut live_swipes, mut live_info) = swipe_arrays(fields).ok_or_else(invalid)?;
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
    write_message(output, fields, &swipes, &info, None)
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
    fn projection(self: Arc<Self>, source_id: u32) -> Box<dyn ByteReader> {
        let records = Records::new(self.file.full_range());
        blocking_reader(ProjectionReader {
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

    async fn record(self: Arc<Self>, record: usize) -> Result<Box<dyn ByteReader>, DomainError> {
        tokio::task::spawn_blocking(move || {
            self.file.check_unchanged().map_err(io_error)?;
            let span = self.record_span(record).map_err(io_error)?;
            Ok(blocking_reader(self.file.range(span)))
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
            self.file.check_unchanged()?;
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
            if records.next(&mut line)?.is_some() {
                parse_header_integrity(&line).map_err(invalid_data)?;
                output.write_all(&line)?;
                output.write_all(b"\n")?;
            }
            while records.next(&mut line)?.is_some() {
                let fields = parse_fields(&line)?;
                if let Some(reference) = fields.get(COLD) {
                    let reference: ColdReference =
                        serde_json::from_str(reference.get()).map_err(invalid_data)?;
                    if reference.source_id != source_id {
                        return Err(invalid_data("Chat commit mixes cold swipe sources"));
                    }
                    original.clear();
                    self.file
                        .range(self.record_span(reference.record)?)
                        .read_to_end(&mut original)?;
                    restore(&fields, &original, &mut output)?;
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
        .map_err(io_error)
    }
}

fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Cold swipes: {error}"))
}
