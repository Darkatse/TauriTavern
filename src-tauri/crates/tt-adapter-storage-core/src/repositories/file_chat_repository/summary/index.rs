use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_repository::ChatSearchResult;

use crate::file_system::persist_file;

use super::super::FileChatRepository;
use super::search::SearchFingerprint;
use super::{FileSignature, summary_cache_key};

const SCHEMA_VERSION: u32 = 4;
const MAX_SEARCH_ENTRIES: usize = 128;
const SNAPSHOT_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub(in crate::repositories::file_chat_repository) struct SummaryCacheEntry {
    pub(super) signature: FileSignature,
    pub(in crate::repositories::file_chat_repository) summary: ChatSearchResult,
    pub(super) preview_unavailable: bool,
    pub(super) fingerprint: Option<SearchFingerprint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ChatStatsCacheEntry {
    pub(super) signature: FileSignature,
    pub(super) date: i64,
}

#[derive(Clone)]
struct SearchCacheEntry {
    version: u64,
    results: Vec<ChatSearchResult>,
}

pub(in crate::repositories::file_chat_repository) struct SummaryCache {
    entries: HashMap<String, SummaryCacheEntry>,
    stats_entries: HashMap<String, ChatStatsCacheEntry>,
    search_cache: HashMap<String, SearchCacheEntry>,
    version: u64,
    index_path: PathBuf,
    backups_dir: PathBuf,
    loaded: bool,
    dirty: bool,
}

/// First line of the streamed JSONL snapshot.
#[derive(Serialize, Deserialize)]
struct SnapshotHeader {
    schema_version: u32,
    version: u64,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SnapshotLineKind {
    Summary,
    Stats,
}

/// Discriminator-only probe; serde ignores the remaining line fields.
#[derive(Deserialize)]
struct SnapshotLineKindProbe {
    kind: SnapshotLineKind,
}

#[derive(Serialize, Deserialize)]
struct SnapshotEntry {
    key: String,
    signature: FileSignature,
    summary: ChatSearchResult,
    preview_unavailable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fingerprint: Option<SearchFingerprint>,
}

#[derive(Serialize, Deserialize)]
struct StatsSnapshotEntry {
    key: String,
    signature: FileSignature,
    date: i64,
}

/// Borrowing serialization views keep the write path clone-free: large
/// `chat_metadata` trees are streamed out field by field, never duplicated.
#[derive(Serialize)]
struct SnapshotEntryRef<'a> {
    kind: SnapshotLineKind,
    key: &'a str,
    signature: FileSignature,
    summary: &'a ChatSearchResult,
    preview_unavailable: bool,
    fingerprint: &'a Option<SearchFingerprint>,
}

#[derive(Serialize)]
struct StatsSnapshotEntryRef<'a> {
    kind: SnapshotLineKind,
    key: &'a str,
    signature: FileSignature,
    date: i64,
}

impl SummaryCache {
    pub(in crate::repositories::file_chat_repository) fn new(
        index_path: PathBuf,
        backups_dir: PathBuf,
    ) -> Self {
        Self {
            entries: HashMap::new(),
            stats_entries: HashMap::new(),
            search_cache: HashMap::new(),
            version: 0,
            index_path,
            backups_dir,
            loaded: false,
            dirty: false,
        }
    }

    fn bump_version(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.search_cache.clear();
    }

    pub(super) fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if !self.index_path.exists() {
            return;
        }

        let file = match File::open(&self.index_path) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(path = %self.index_path.display(), %error, "Failed to read chat summary index");
                return;
            }
        };
        let mut reader = BufReader::with_capacity(SNAPSHOT_READ_BUFFER_BYTES, file);
        let header = match read_snapshot_header(&mut reader) {
            Ok(header) => header,
            Err(error) => {
                tracing::warn!(path = %self.index_path.display(), error = %error, "Failed to parse chat summary index header");
                return;
            }
        };
        let Some(header) = header else {
            return;
        };
        if header.schema_version != SCHEMA_VERSION {
            tracing::warn!(
                schema_version = header.schema_version,
                expected = SCHEMA_VERSION,
                "Skipping incompatible chat summary index"
            );
            // Rewrite the stale index in the current schema on the next flush.
            self.dirty = true;
            return;
        }
        self.version = header.version;

        let mut entries = HashMap::new();
        let mut stats_entries = HashMap::new();
        let mut filtered_backups = false;
        if let Err(error) = load_snapshot_lines(
            &mut reader,
            &self.backups_dir,
            &mut entries,
            &mut stats_entries,
            &mut filtered_backups,
        ) {
            tracing::warn!(path = %self.index_path.display(), error = %error, "Failed to load chat summary index; rebuilding from scans");
            return;
        }
        self.dirty = filtered_backups;
        self.entries = entries;
        self.stats_entries = stats_entries;
    }

    pub(super) fn get(&self, key: &str) -> Option<&SummaryCacheEntry> {
        self.entries.get(key)
    }

    pub(super) fn set(&mut self, key: String, entry: SummaryCacheEntry) {
        self.stats_entries.remove(&key);
        self.entries.insert(key, entry);
        self.bump_version();
        self.dirty = true;
    }

    pub(super) fn get_stats(
        &self,
        key: &str,
        signature: FileSignature,
    ) -> Option<ChatStatsCacheEntry> {
        if let Some(entry) = self.entries.get(key)
            && entry.signature == signature
        {
            return Some(ChatStatsCacheEntry {
                signature,
                date: entry.summary.date,
            });
        }
        self.stats_entries
            .get(key)
            .filter(|entry| entry.signature == signature)
            .cloned()
    }

    pub(super) fn set_stats(&mut self, key: String, entry: ChatStatsCacheEntry) {
        if self
            .entries
            .get(&key)
            .is_some_and(|summary| summary.signature == entry.signature)
        {
            return;
        }
        self.stats_entries.insert(key, entry);
        self.bump_version();
        self.dirty = true;
    }

    pub(super) fn remove(&mut self, key: &str) {
        let removed_summary = self.entries.remove(key).is_some();
        let removed_stats = self.stats_entries.remove(key).is_some();
        if removed_summary || removed_stats {
            self.dirty = true;
        }
        self.bump_version();
    }

    pub(super) fn clear(&mut self) {
        if !self.entries.is_empty() || !self.stats_entries.is_empty() {
            self.entries.clear();
            self.stats_entries.clear();
            self.dirty = true;
        }
        self.bump_version();
    }

    pub(super) fn get_search_results(&self, key: &str) -> Option<Vec<ChatSearchResult>> {
        self.search_cache
            .get(key)
            .filter(|entry| entry.version == self.version)
            .map(|entry| entry.results.clone())
    }

    pub(super) fn set_search_results(&mut self, key: String, results: Vec<ChatSearchResult>) {
        if self.search_cache.len() >= MAX_SEARCH_ENTRIES {
            self.search_cache.clear();
        }
        self.search_cache.insert(
            key,
            SearchCacheEntry {
                version: self.version,
                results,
            },
        );
    }
}

impl FileChatRepository {
    pub(in crate::repositories::file_chat_repository) async fn clear_summary_cache(&self) {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded();
        cache.clear();
    }

    pub async fn clear_chat_summary_index(&self) {
        {
            let mut cache = self.summary_cache.lock().await;
            cache.ensure_loaded();
            cache.clear();
        }
        self.flush_summary_index_best_effort().await;
    }

    pub(in crate::repositories::file_chat_repository) async fn remove_summary_cache_for_path(
        &self,
        path: &Path,
    ) {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded();
        cache.remove(&summary_cache_key(path));
    }

    pub(in crate::repositories::file_chat_repository) async fn get_cached_search_results(
        &self,
        key: &str,
    ) -> Option<Vec<ChatSearchResult>> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded();
        cache.get_search_results(key)
    }

    pub(in crate::repositories::file_chat_repository) async fn cache_search_results(
        &self,
        key: String,
        results: Vec<ChatSearchResult>,
    ) {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded();
        cache.set_search_results(key, results);
    }

    pub(in crate::repositories::file_chat_repository) async fn flush_summary_index_if_needed(
        &self,
    ) -> Result<(), DomainError> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded();
        if !cache.dirty {
            return Ok(());
        }

        let index_path = cache.index_path.clone();
        let temp_path = index_path.with_extension("json.tmp");
        if let Some(parent) = index_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat summary index directory {:?}: {error}",
                    parent
                ))
            })?;
        }
        // Streaming per-entry serialization: each line is encoded into a bounded
        // buffer — at most one entry, never a clone of the whole metadata-heavy
        // cache — then written asynchronously and published with the standard
        // sync + atomic rename helper.
        let write_result: Result<(), DomainError> = async {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create staged chat summary index {:?}: {error}",
                        temp_path
                    ))
                })?;
            let mut chunk = encode_snapshot_header_line(cache.version)?;
            chunk.push(b'\n');
            file.write_all(&chunk).await.map_err(snapshot_io_error)?;
            for (key, entry) in cache.entries.iter() {
                let chunk = encode_summary_line(key, entry)?;
                file.write_all(&chunk).await.map_err(snapshot_io_error)?;
            }
            for (key, entry) in cache.stats_entries.iter() {
                let chunk = encode_stats_line(key, entry)?;
                file.write_all(&chunk).await.map_err(snapshot_io_error)?;
            }
            file.flush().await.map_err(snapshot_io_error)?;
            persist_file(file, &temp_path, &index_path).await
        }
        .await;
        match write_result {
            Ok(()) => {
                cache.dirty = false;
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_path).await;
                Err(error)
            }
        }
    }

    pub(in crate::repositories::file_chat_repository) async fn flush_summary_index_best_effort(
        &self,
    ) {
        if let Err(error) = self.flush_summary_index_if_needed().await {
            tracing::warn!(%error, "Failed to persist chat summary index");
        }
    }
}

fn snapshot_json_error(error: serde_json::Error) -> DomainError {
    DomainError::InternalError(format!("Failed to parse chat summary index line: {error}"))
}

fn snapshot_serialize_error(error: serde_json::Error) -> DomainError {
    DomainError::InternalError(format!("Failed to serialize chat summary index: {error}"))
}

fn snapshot_io_error(error: io::Error) -> DomainError {
    DomainError::InternalError(format!("Failed to write chat summary index: {error}"))
}

fn encode_snapshot_header_line(version: u64) -> Result<Vec<u8>, DomainError> {
    let mut bytes = Vec::new();
    serde_json::to_writer(
        &mut bytes,
        &SnapshotHeader {
            schema_version: SCHEMA_VERSION,
            version,
        },
    )
    .map_err(snapshot_serialize_error)?;
    Ok(bytes)
}

fn encode_summary_line(key: &str, entry: &SummaryCacheEntry) -> Result<Vec<u8>, DomainError> {
    let mut bytes = Vec::new();
    serde_json::to_writer(
        &mut bytes,
        &SnapshotEntryRef {
            kind: SnapshotLineKind::Summary,
            key,
            signature: entry.signature,
            summary: &entry.summary,
            preview_unavailable: entry.preview_unavailable,
            fingerprint: &entry.fingerprint,
        },
    )
    .map_err(snapshot_serialize_error)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn encode_stats_line(key: &str, entry: &ChatStatsCacheEntry) -> Result<Vec<u8>, DomainError> {
    let mut bytes = Vec::new();
    serde_json::to_writer(
        &mut bytes,
        &StatsSnapshotEntryRef {
            kind: SnapshotLineKind::Stats,
            key,
            signature: entry.signature,
            date: entry.date,
        },
    )
    .map_err(snapshot_serialize_error)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_snapshot_header(reader: &mut impl BufRead) -> Result<Option<SnapshotHeader>, DomainError> {
    let mut line = String::new();
    let bytes_read = reader
        .read_line(&mut line)
        .map_err(snapshot_io_error)?;
    if bytes_read == 0 {
        return Ok(None);
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to parse chat summary index header: {error}"
            ))
        })
}

fn load_snapshot_lines(
    reader: &mut impl BufRead,
    backups_dir: &Path,
    entries: &mut HashMap<String, SummaryCacheEntry>,
    stats_entries: &mut HashMap<String, ChatStatsCacheEntry>,
    filtered_backups: &mut bool,
) -> Result<(), DomainError> {
    for line in reader.lines() {
        let line = line.map_err(snapshot_io_error)?;
        if line.trim().is_empty() {
            continue;
        }
        let kind = serde_json::from_str::<SnapshotLineKindProbe>(&line)
            .map_err(snapshot_json_error)?
            .kind;
        match kind {
            SnapshotLineKind::Summary => {
                let entry: SnapshotEntry = serde_json::from_str(&line).map_err(snapshot_json_error)?;
                if Path::new(&entry.key).starts_with(backups_dir) {
                    *filtered_backups = true;
                    continue;
                }
                let mut fingerprint = entry.fingerprint;
                if let Some(value) = fingerprint.as_mut() {
                    value.normalize_len();
                }
                entries.insert(
                    entry.key,
                    SummaryCacheEntry {
                        signature: entry.signature,
                        summary: entry.summary,
                        preview_unavailable: entry.preview_unavailable,
                        fingerprint,
                    },
                );
            }
            SnapshotLineKind::Stats => {
                let entry: StatsSnapshotEntry =
                    serde_json::from_str(&line).map_err(snapshot_json_error)?;
                if Path::new(&entry.key).starts_with(backups_dir) {
                    *filtered_backups = true;
                    continue;
                }
                stats_entries.insert(
                    entry.key,
                    ChatStatsCacheEntry {
                        signature: entry.signature,
                        date: entry.date,
                    },
                );
            }
        }
    }
    Ok(())
}
