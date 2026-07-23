use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

use crate::chat_directory_identity::{self, SharedChatAliasStore};
use crate::file_system::list_files_with_extension;
use tt_domain::errors::DomainError;
use tt_domain::models::chat::{parse_message_timestamp_value, strip_jsonl_extension};
use tt_ports::repositories::chat_repository::ChatSearchResult;

use super::FileChatRepository;
use super::backup_codec::{BackupFormat, open_decoded_backup};

const INDEX_SCHEMA_VERSION: u32 = 1;
const FINGERPRINT_WORDS: usize = 64; // 4096 bits
const MAX_SEARCH_CACHE_ENTRIES: usize = 128;
const SUMMARY_SCAN_BUFFER_BYTES: usize = 64 * 1024;
const MAX_CONCURRENT_CHAT_STATS_READS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct FileSignature {
    pub size: u64,
    pub modified_millis: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct SearchFingerprint {
    bits: Vec<u64>,
}

impl SearchFingerprint {
    pub(super) fn new() -> Self {
        Self {
            bits: vec![0; FINGERPRINT_WORDS],
        }
    }

    fn normalize_len(&mut self) {
        if self.bits.len() != FINGERPRINT_WORDS {
            self.bits.resize(FINGERPRINT_WORDS, 0);
        }
    }

    fn set_hashed(&mut self, hash: u64) {
        let bit_count = (FINGERPRINT_WORDS as u64) * 64;
        let bit_index = (hash % bit_count) as usize;
        let word_index = bit_index / 64;
        let offset = bit_index % 64;
        self.bits[word_index] |= 1u64 << offset;
    }

    fn has_hashed(&self, hash: u64) -> bool {
        let bit_count = (FINGERPRINT_WORDS as u64) * 64;
        let bit_index = (hash % bit_count) as usize;
        let word_index = bit_index / 64;
        let offset = bit_index % 64;
        self.bits
            .get(word_index)
            .map(|word| (word & (1u64 << offset)) != 0)
            .unwrap_or(false)
    }

    fn hash_trigram(chars: [char; 3]) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (chars[0] as u32).hash(&mut hasher);
        (chars[1] as u32).hash(&mut hasher);
        (chars[2] as u32).hash(&mut hasher);
        hasher.finish()
    }

    fn visit_trigram_hashes(value: &str, mut visit: impl FnMut(u64)) -> bool {
        let mut chars = value.chars().flat_map(|ch| ch.to_lowercase());

        let Some(mut first) = chars.next() else {
            return false;
        };
        let Some(mut second) = chars.next() else {
            return false;
        };
        let Some(mut third) = chars.next() else {
            return false;
        };

        loop {
            visit(Self::hash_trigram([first, second, third]));
            let Some(next) = chars.next() else {
                return true;
            };
            first = second;
            second = third;
            third = next;
        }
    }

    pub(super) fn add_text(&mut self, value: &str) {
        self.normalize_len();
        Self::visit_trigram_hashes(value, |hash| self.set_hashed(hash));
    }

    fn might_match_fragment(&self, fragment: &str) -> bool {
        if fragment.chars().count() < 3 {
            return true;
        }

        let mut matches = true;
        let saw_trigram = Self::visit_trigram_hashes(fragment, |hash| {
            if !self.has_hashed(hash) {
                matches = false;
            }
        });

        !saw_trigram || matches
    }

    pub(super) fn might_match_fragments(&self, fragments: &[String]) -> bool {
        fragments
            .iter()
            .all(|fragment| self.might_match_fragment(fragment))
    }
}

#[derive(Clone, Debug)]
pub(super) struct SummaryCacheEntry {
    pub signature: FileSignature,
    pub summary: ChatSearchResult,
    pub fingerprint: Option<SearchFingerprint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChatStatsCacheEntry {
    signature: FileSignature,
    date: i64,
}

struct SummaryFileScan {
    line_count: usize,
    first_non_empty: Option<String>,
    last_non_empty: Option<String>,
    fingerprint: Option<SearchFingerprint>,
}

#[derive(Default)]
struct SummaryLineByteScan {
    line_count: usize,
    first_non_empty: Option<Vec<u8>>,
    last_non_empty: Option<Vec<u8>>,
}

#[derive(Clone)]
struct SearchCacheEntry {
    version: u64,
    results: Vec<ChatSearchResult>,
}

pub(super) struct SummaryCache {
    entries: HashMap<String, SummaryCacheEntry>,
    stats_entries: HashMap<String, ChatStatsCacheEntry>,
    search_cache: HashMap<String, SearchCacheEntry>,
    version: u64,
    index_path: PathBuf,
    loaded: bool,
    dirty: bool,
}

#[derive(Serialize, Deserialize)]
struct SummaryIndexSnapshot {
    schema_version: u32,
    version: u64,
    entries: Vec<SummaryIndexSnapshotEntry>,
    #[serde(default)]
    stats_entries: Vec<SummaryStatsSnapshotEntry>,
}

#[derive(Serialize, Deserialize)]
struct SummaryIndexSnapshotEntry {
    key: String,
    signature: FileSignature,
    summary: ChatSearchResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fingerprint: Option<SearchFingerprint>,
}

#[derive(Serialize, Deserialize)]
struct SummaryStatsSnapshotEntry {
    key: String,
    signature: FileSignature,
    date: i64,
}

impl SummaryCache {
    pub(super) fn new(index_path: PathBuf) -> Self {
        Self {
            entries: HashMap::new(),
            stats_entries: HashMap::new(),
            search_cache: HashMap::new(),
            version: 0,
            index_path,
            loaded: false,
            dirty: false,
        }
    }

    pub(super) fn index_path(&self) -> &Path {
        &self.index_path
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(super) fn mark_clean(&mut self) {
        self.dirty = false;
    }

    fn bump_version(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.search_cache.clear();
    }

    pub(super) fn ensure_loaded(&mut self) -> Result<(), DomainError> {
        if self.loaded {
            return Ok(());
        }

        self.loaded = true;
        if !self.index_path.exists() {
            return Ok(());
        }

        let bytes = match std::fs::read(&self.index_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(
                    "Failed to read chat summary index {:?}: {}",
                    self.index_path,
                    error
                );
                return Ok(());
            }
        };

        let snapshot: SummaryIndexSnapshot = match serde_json::from_slice(&bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    "Failed to parse chat summary index {:?}: {}",
                    self.index_path,
                    error
                );
                return Ok(());
            }
        };

        if snapshot.schema_version != INDEX_SCHEMA_VERSION {
            tracing::warn!(
                "Skipping incompatible chat summary index schema {} (expected {})",
                snapshot.schema_version,
                INDEX_SCHEMA_VERSION
            );
            return Ok(());
        }

        self.version = snapshot.version;
        for entry in snapshot.entries {
            let mut fingerprint = entry.fingerprint;
            if let Some(value) = fingerprint.as_mut() {
                value.normalize_len();
            }
            self.entries.insert(
                entry.key,
                SummaryCacheEntry {
                    signature: entry.signature,
                    summary: entry.summary,
                    fingerprint,
                },
            );
        }
        for entry in snapshot.stats_entries {
            self.stats_entries.insert(
                entry.key,
                ChatStatsCacheEntry {
                    signature: entry.signature,
                    date: entry.date,
                },
            );
        }

        Ok(())
    }

    pub(super) fn serialize_snapshot(&self) -> Result<Vec<u8>, DomainError> {
        let snapshot = SummaryIndexSnapshot {
            schema_version: INDEX_SCHEMA_VERSION,
            version: self.version,
            entries: self
                .entries
                .iter()
                .map(|(key, entry)| SummaryIndexSnapshotEntry {
                    key: key.clone(),
                    signature: entry.signature,
                    summary: entry.summary.clone(),
                    fingerprint: entry.fingerprint.clone(),
                })
                .collect(),
            stats_entries: self
                .stats_entries
                .iter()
                .map(|(key, entry)| SummaryStatsSnapshotEntry {
                    key: key.clone(),
                    signature: entry.signature,
                    date: entry.date,
                })
                .collect(),
        };

        serde_json::to_vec(&snapshot).map_err(|error| {
            DomainError::InternalError(format!("Failed to serialize chat summary index: {}", error))
        })
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

    fn get_stats(&self, key: &str, signature: FileSignature) -> Option<ChatStatsCacheEntry> {
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

    fn set_stats(&mut self, key: String, entry: ChatStatsCacheEntry) {
        if let Some(summary) = self.entries.get(&key)
            && summary.signature == entry.signature
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
        self.search_cache.get(key).and_then(|entry| {
            if entry.version == self.version {
                Some(entry.results.clone())
            } else {
                None
            }
        })
    }

    pub(super) fn set_search_results(&mut self, key: String, results: Vec<ChatSearchResult>) {
        if self.search_cache.len() >= MAX_SEARCH_CACHE_ENTRIES {
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

#[derive(Clone, Debug)]
pub(super) struct ChatFileDescriptor {
    pub character_name: String,
    pub file_name: String,
    pub path: PathBuf,
}

impl FileChatRepository {
    pub async fn calculate_character_chat_stats(
        &self,
        character_name: &str,
    ) -> Result<(u64, i64), DomainError> {
        let mut results = self
            .calculate_character_chat_stats_batch(vec![character_name.to_string()])
            .await?;
        let (_, result) = results.pop().ok_or_else(|| {
            DomainError::InternalError("Character chat stats batch returned no result".to_string())
        })?;
        result
    }

    pub async fn calculate_character_chat_stats_batch(
        &self,
        character_names: Vec<String>,
    ) -> Result<Vec<(String, Result<(u64, i64), DomainError>)>, DomainError> {
        let mut results = Vec::with_capacity(character_names.len());
        let semaphore = Arc::new(Semaphore::new(Self::chat_stats_parallelism()));
        let mut jobs = JoinSet::new();

        for character_name in character_names {
            let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
                DomainError::InternalError("Character chat stats scanner gate closed".to_string())
            })?;
            let characters_dir = self.characters_dir.clone();
            let chats_dir = self.chats_dir.clone();
            let chat_aliases = self.chat_aliases.clone();
            let summary_cache = self.summary_cache.clone();

            jobs.spawn(async move {
                let _permit = permit;
                let result = Self::calculate_character_chat_stats_from_parts(
                    &characters_dir,
                    &chats_dir,
                    &chat_aliases,
                    &summary_cache,
                    &character_name,
                )
                .await;
                (character_name, result)
            });
        }

        while let Some(joined) = jobs.join_next().await {
            results.push(joined.map_err(|error| {
                DomainError::InternalError(format!(
                    "Character chat stats scanner failed: {}",
                    error
                ))
            })?);
        }

        if let Err(error) = self.flush_summary_index_if_needed().await {
            tracing::warn!("Failed to persist chat stats index: {}", error);
        }

        Ok(results)
    }

    async fn calculate_character_chat_stats_from_parts(
        characters_dir: &Path,
        chats_dir: &Path,
        chat_aliases: &SharedChatAliasStore,
        summary_cache: &Arc<Mutex<SummaryCache>>,
        character_name: &str,
    ) -> Result<(u64, i64), DomainError> {
        let dir_key = chat_directory_identity::resolve_character_chat_dir_key(
            characters_dir,
            chats_dir,
            chat_aliases,
            character_name,
        )
        .await?;
        let chat_dir = chats_dir.join(dir_key);
        let files = list_files_with_extension(&chat_dir, "jsonl").await?;

        let mut total_size = 0;
        let mut latest_chat_date = 0;
        for path in files {
            let Some(file_name) = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
            else {
                continue;
            };
            let descriptor = ChatFileDescriptor {
                character_name: character_name.to_string(),
                file_name,
                path,
            };
            let entry = Self::get_chat_stats_entry(summary_cache, &descriptor).await?;
            total_size += entry.signature.size;
            latest_chat_date = latest_chat_date.max(entry.date);
        }

        Ok((total_size, latest_chat_date))
    }

    pub(super) async fn get_chat_stats_date(
        summary_cache: &Arc<Mutex<SummaryCache>>,
        descriptor: &ChatFileDescriptor,
    ) -> Result<i64, DomainError> {
        Ok(Self::get_chat_stats_entry(summary_cache, descriptor)
            .await?
            .date)
    }

    async fn get_chat_stats_entry(
        summary_cache: &Arc<Mutex<SummaryCache>>,
        descriptor: &ChatFileDescriptor,
    ) -> Result<ChatStatsCacheEntry, DomainError> {
        let metadata = fs::metadata(&descriptor.path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat metadata {:?}: {}",
                descriptor.path, error
            ))
        })?;
        let signature = Self::file_signature_from_metadata(&metadata);
        let cache_key = Self::summary_cache_key(&descriptor.path);

        {
            let mut cache = summary_cache.lock().await;
            cache.ensure_loaded()?;
            if let Some(entry) = cache.get_stats(&cache_key, signature) {
                return Ok(entry);
            }
        }

        let mut file = File::open(&descriptor.path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to open chat file {:?}: {}",
                descriptor.path, error
            ))
        })?;
        let metadata = file.metadata().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat metadata {:?}: {}",
                descriptor.path, error
            ))
        })?;
        let signature = Self::file_signature_from_metadata(&metadata);
        {
            let mut cache = summary_cache.lock().await;
            cache.ensure_loaded()?;
            if let Some(entry) = cache.get_stats(&cache_key, signature) {
                return Ok(entry);
            }
        }

        let date = Self::scan_chat_stats_date(&mut file, &descriptor.path, signature).await?;
        let entry = ChatStatsCacheEntry { signature, date };

        {
            let mut cache = summary_cache.lock().await;
            cache.ensure_loaded()?;
            cache.set_stats(cache_key, entry.clone());
        }

        Ok(entry)
    }

    async fn scan_chat_stats_date(
        file: &mut File,
        path: &Path,
        signature: FileSignature,
    ) -> Result<i64, DomainError> {
        let Some(line) = Self::read_last_non_empty_line(file, path, signature.size).await? else {
            return Ok(signature.modified_millis);
        };
        let last_message = serde_json::from_slice::<Value>(&line).ok();
        let parsed_date = parse_message_timestamp_value(
            last_message
                .as_ref()
                .and_then(|message| message.get("send_date")),
        );

        if parsed_date > 0 {
            Ok(parsed_date)
        } else {
            Ok(signature.modified_millis)
        }
    }

    async fn read_last_non_empty_line(
        file: &mut File,
        path: &Path,
        file_size: u64,
    ) -> Result<Option<Vec<u8>>, DomainError> {
        if file_size == 0 {
            return Ok(None);
        }

        let mut position = file_size;
        let mut reversed_line = Vec::new();

        while position > 0 {
            let read_len = position.min(SUMMARY_SCAN_BUFFER_BYTES as u64) as usize;
            position -= read_len as u64;
            file.seek(SeekFrom::Start(position))
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to seek chat file {:?}: {}",
                        path, error
                    ))
                })?;

            let mut buffer = vec![0u8; read_len];
            file.read_exact(&mut buffer).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat file {:?}: {}",
                    path, error
                ))
            })?;

            for &byte in buffer.iter().rev() {
                if byte == b'\n' {
                    if let Some(line) = Self::finish_reversed_stats_line(path, &mut reversed_line)?
                    {
                        return Ok(Some(line));
                    }
                } else {
                    reversed_line.push(byte);
                }
            }
        }

        Self::finish_reversed_stats_line(path, &mut reversed_line)
    }

    fn finish_reversed_stats_line(
        path: &Path,
        reversed_line: &mut Vec<u8>,
    ) -> Result<Option<Vec<u8>>, DomainError> {
        let mut line = std::mem::take(reversed_line);
        line.reverse();
        Self::trim_trailing_carriage_returns(&mut line);
        let is_empty = std::str::from_utf8(&line)
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to decode chat line in chat file {:?}: {}",
                    path, error
                ))
            })?
            .trim()
            .is_empty();

        if is_empty { Ok(None) } else { Ok(Some(line)) }
    }

    pub(super) fn chat_stats_parallelism() -> usize {
        std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(4)
            .clamp(1, MAX_CONCURRENT_CHAT_STATS_READS)
    }

    async fn list_character_chat_directory_keys(&self) -> Result<Vec<String>, DomainError> {
        if !self.characters_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&self.characters_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read characters directory {:?}: {}",
                self.characters_dir, error
            ))
        })?;

        let mut keys = HashSet::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read characters directory entry {:?}: {}",
                self.characters_dir, error
            ))
        })? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let is_character_card = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("png"))
                .unwrap_or(false);
            if !is_character_card {
                continue;
            }

            let Some(file_stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if file_stem.is_empty() {
                continue;
            }
            keys.insert(file_stem.to_string());
        }

        let mut sorted_keys: Vec<String> = keys.into_iter().collect();
        sorted_keys.sort();
        Ok(sorted_keys)
    }

    pub(super) async fn clear_summary_cache(&self) {
        let mut cache = self.summary_cache.lock().await;
        if cache.ensure_loaded().is_err() {
            return;
        }
        cache.clear();
    }

    pub async fn clear_chat_summary_index(&self) -> Result<(), DomainError> {
        {
            let mut cache = self.summary_cache.lock().await;
            cache.ensure_loaded()?;
            cache.clear();
        }
        self.flush_summary_index_if_needed().await
    }

    pub(super) async fn remove_summary_cache_for_path(&self, path: &Path) {
        let mut cache = self.summary_cache.lock().await;
        if cache.ensure_loaded().is_err() {
            return;
        }
        cache.remove(&Self::summary_cache_key(path));
    }

    pub(super) async fn get_cached_search_results(
        &self,
        key: &str,
    ) -> Option<Vec<ChatSearchResult>> {
        let mut cache = self.summary_cache.lock().await;
        if cache.ensure_loaded().is_err() {
            return None;
        }
        cache.get_search_results(key)
    }

    pub(super) async fn cache_search_results(&self, key: String, results: Vec<ChatSearchResult>) {
        let mut cache = self.summary_cache.lock().await;
        if cache.ensure_loaded().is_err() {
            return;
        }
        cache.set_search_results(key, results);
    }

    async fn ensure_summary_index_loaded(&self) -> Result<(), DomainError> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded()
    }

    pub(super) async fn flush_summary_index_if_needed(&self) -> Result<(), DomainError> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded()?;
        if !cache.is_dirty() {
            return Ok(());
        }

        let index_path = cache.index_path().to_path_buf();
        let bytes = cache.serialize_snapshot()?;

        if let Some(parent) = index_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat summary index directory {:?}: {}",
                    parent, error
                ))
            })?;
        }

        fs::write(&index_path, bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write chat summary index {:?}: {}",
                index_path, error
            ))
        })?;

        cache.mark_clean();

        Ok(())
    }

    pub(super) async fn list_character_chat_files(
        &self,
        character_filter: Option<&str>,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError> {
        self.ensure_directory_exists().await?;

        if let Some(character_name) = character_filter {
            let dir = self.resolve_character_chat_dir(character_name).await?;
            let files = list_files_with_extension(&dir, "jsonl").await?;
            return Ok(files
                .into_iter()
                .filter_map(|path| {
                    let file_name = path.file_name()?.to_str()?.to_string();
                    Some(ChatFileDescriptor {
                        character_name: character_name.to_string(),
                        file_name,
                        path,
                    })
                })
                .collect());
        }

        let mut descriptors = Vec::new();
        for character_name in self.list_character_chat_directory_keys().await? {
            let path = self.resolve_character_chat_dir(&character_name).await?;
            let files = list_files_with_extension(&path, "jsonl").await?;
            descriptors.extend(files.into_iter().filter_map(|file_path| {
                let file_name = file_path.file_name()?.to_str()?.to_string();
                Some(ChatFileDescriptor {
                    character_name: character_name.clone(),
                    file_name,
                    path: file_path,
                })
            }));
        }

        let root_chat_files = list_files_with_extension(&self.chats_dir, "jsonl").await?;
        descriptors.extend(root_chat_files.into_iter().filter_map(|path| {
            let file_name = path.file_name()?.to_str()?.to_string();
            Some(ChatFileDescriptor {
                character_name: String::new(),
                file_name,
                path,
            })
        }));

        Ok(descriptors)
    }

    pub(super) async fn list_group_chat_files(
        &self,
        chat_ids: Option<&[String]>,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError> {
        self.ensure_directory_exists().await?;

        if let Some(chat_ids) = chat_ids {
            let id_set: HashSet<String> = chat_ids
                .iter()
                .map(|id| strip_jsonl_extension(id).to_string())
                .collect();

            let mut descriptors = Vec::new();
            for id in id_set {
                let path = self.get_group_chat_path(&id)?;
                if !path.exists() {
                    continue;
                }
                descriptors.push(ChatFileDescriptor {
                    character_name: String::new(),
                    file_name: Self::normalize_jsonl_file_name(&id)?,
                    path,
                });
            }
            return Ok(descriptors);
        }

        let files = list_files_with_extension(&self.group_chats_dir, "jsonl").await?;
        Ok(files
            .into_iter()
            .filter_map(|path| {
                let file_name = path.file_name()?.to_str()?.to_string();
                Some(ChatFileDescriptor {
                    character_name: String::new(),
                    file_name,
                    path,
                })
            })
            .collect())
    }

    pub(super) async fn get_chat_summary_entry(
        &self,
        descriptor: &ChatFileDescriptor,
        require_fingerprint: bool,
    ) -> Result<SummaryCacheEntry, DomainError> {
        self.ensure_summary_index_loaded().await?;

        let metadata = fs::metadata(&descriptor.path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read chat metadata {:?}: {}",
                descriptor.path, error
            ))
        })?;
        let signature = Self::file_signature_from_metadata(&metadata);
        let cache_key = Self::summary_cache_key(&descriptor.path);

        {
            let cache = self.summary_cache.lock().await;
            if let Some(entry) = cache.get(&cache_key) {
                let has_required_fingerprint = !require_fingerprint || entry.fingerprint.is_some();
                if entry.signature == signature && has_required_fingerprint {
                    return Ok(entry.clone());
                }
            }
        }

        let scanned = self
            .scan_chat_summary_file(
                &descriptor.path,
                &descriptor.character_name,
                &descriptor.file_name,
                signature,
                require_fingerprint,
            )
            .await?;

        {
            let mut cache = self.summary_cache.lock().await;
            cache.set(cache_key, scanned.clone());
        }

        Ok(scanned)
    }

    pub(super) async fn get_chat_summary(
        &self,
        descriptor: &ChatFileDescriptor,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        let mut summary = self
            .get_chat_summary_entry(descriptor, false)
            .await?
            .summary;
        if !include_metadata {
            summary.chat_metadata = None;
        }
        Ok(summary)
    }

    pub(super) async fn get_character_chat_summary_internal(
        &self,
        character_name: &str,
        file_name: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        self.ensure_directory_exists().await?;

        let path = self
            .resolve_character_chat_path(character_name, file_name)
            .await?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat not found: {}/{}",
                character_name, file_name
            )));
        }

        let descriptor = ChatFileDescriptor {
            character_name: character_name.to_string(),
            file_name: Self::normalize_jsonl_file_name(file_name)?,
            path,
        };

        self.get_chat_summary(&descriptor, include_metadata).await
    }

    pub(super) async fn get_group_chat_summary_internal(
        &self,
        chat_id: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError> {
        self.ensure_directory_exists().await?;

        let path = self.get_group_chat_path(chat_id)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "Group chat not found: {}",
                chat_id
            )));
        }

        let descriptor = ChatFileDescriptor {
            character_name: String::new(),
            file_name: Self::normalize_jsonl_file_name(chat_id)?,
            path,
        };

        self.get_chat_summary(&descriptor, include_metadata).await
    }

    pub(super) fn file_stem_matches_all(file_stem: &str, fragments: &[String]) -> bool {
        if fragments.is_empty() {
            return true;
        }
        let lowered = file_stem.to_lowercase();
        fragments.iter().all(|fragment| lowered.contains(fragment))
    }

    pub(super) async fn file_matches_query(
        &self,
        path: &Path,
        file_stem: &str,
        fragments: &[String],
    ) -> Result<bool, DomainError> {
        if fragments.is_empty() {
            return Ok(true);
        }

        let mut matches = vec![false; fragments.len()];
        let file_stem_lower = file_stem.to_lowercase();
        for (index, fragment) in fragments.iter().enumerate() {
            if file_stem_lower.contains(fragment) {
                matches[index] = true;
            }
        }

        if matches.iter().all(|matched| *matched) {
            return Ok(true);
        }

        let file = File::open(path).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to open chat file {:?}: {}", path, error))
        })?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await.map_err(|error| {
            DomainError::InternalError(format!("Failed to read chat file {:?}: {}", path, error))
        })? {
            if line.trim().is_empty() {
                continue;
            }

            let lower = line.to_lowercase();
            for (index, fragment) in fragments.iter().enumerate() {
                if !matches[index] && lower.contains(fragment) {
                    matches[index] = true;
                }
            }

            if matches.iter().all(|matched| *matched) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub(super) fn normalize_search_query(query: &str) -> String {
        query
            .trim()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(super) fn search_fragments(query: &str) -> Vec<String> {
        query
            .trim()
            .to_lowercase()
            .split_whitespace()
            .filter(|fragment| !fragment.is_empty())
            .map(ToString::to_string)
            .collect()
    }

    fn summary_cache_key(path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    pub(super) fn file_signature_from_metadata(metadata: &std::fs::Metadata) -> FileSignature {
        let modified_millis = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        FileSignature {
            size: metadata.len(),
            modified_millis,
        }
    }

    async fn scan_chat_summary_file(
        &self,
        path: &Path,
        fallback_character_name: &str,
        fallback_file_name: &str,
        signature: FileSignature,
        include_fingerprint: bool,
    ) -> Result<SummaryCacheEntry, DomainError> {
        let scan = if include_fingerprint {
            Self::scan_chat_summary_lines_with_fingerprint(path, fallback_file_name).await?
        } else {
            Self::scan_chat_summary_lines_without_fingerprint(path).await?
        };

        let header = scan
            .first_non_empty
            .as_deref()
            .and_then(|line| serde_json::from_str::<Value>(line).ok())
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        let last_message = scan
            .last_non_empty
            .as_deref()
            .and_then(|line| serde_json::from_str::<Value>(line).ok())
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        let character_name = header
            .get("character_name")
            .and_then(Value::as_str)
            .filter(|name| {
                let trimmed = name.trim();
                !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unused")
            })
            .unwrap_or(fallback_character_name)
            .to_string();

        let chat_id = header
            .get("chat_metadata")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("chat_id_hash"))
            .and_then(|value| {
                value
                    .as_u64()
                    .map(|number| number.to_string())
                    .or_else(|| value.as_i64().map(|number| number.to_string()))
                    .or_else(|| value.as_str().map(ToString::to_string))
            });

        let metadata = header.get("chat_metadata").cloned();
        let message_count = scan.line_count.saturating_sub(1);
        let preview = last_message
            .get("mes")
            .and_then(Value::as_str)
            .map(Self::preview_message_text)
            .unwrap_or_default();
        let parsed_date = parse_message_timestamp_value(last_message.get("send_date"));
        let date = if parsed_date > 0 {
            parsed_date
        } else {
            signature.modified_millis
        };

        Ok(SummaryCacheEntry {
            signature,
            summary: ChatSearchResult {
                character_name,
                file_name: Self::normalize_jsonl_file_name(fallback_file_name)?,
                file_size: signature.size,
                message_count,
                preview,
                date,
                chat_id,
                chat_metadata: metadata,
            },
            fingerprint: scan.fingerprint,
        })
    }

    async fn scan_chat_summary_lines_with_fingerprint(
        path: &Path,
        fallback_file_name: &str,
    ) -> Result<SummaryFileScan, DomainError> {
        let file = File::open(path).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to open chat file {:?}: {}", path, error))
        })?;
        let mut reader = BufReader::new(file);

        let mut line_count: usize = 0;
        let mut first_non_empty: Option<String> = None;
        let mut last_non_empty = String::new();
        let mut has_last_non_empty = false;
        let mut fingerprint = SearchFingerprint::new();
        fingerprint.add_text(strip_jsonl_extension(fallback_file_name));

        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat file {:?}: {}",
                    path, error
                ))
            })?;
            if bytes_read == 0 {
                break;
            }

            let line_text = line.trim_end_matches(['\r', '\n']);
            if line_text.trim().is_empty() {
                continue;
            }

            line_count += 1;
            if first_non_empty.is_none() {
                first_non_empty = Some(line_text.to_string());
            }
            fingerprint.add_text(line_text);
            last_non_empty.clear();
            last_non_empty.push_str(line_text);
            has_last_non_empty = true;
        }

        Ok(SummaryFileScan {
            line_count,
            first_non_empty,
            last_non_empty: has_last_non_empty.then_some(last_non_empty),
            fingerprint: Some(fingerprint),
        })
    }

    async fn scan_chat_summary_lines_without_fingerprint(
        path: &Path,
    ) -> Result<SummaryFileScan, DomainError> {
        let bytes = Self::scan_summary_line_bytes(path).await?;

        Ok(SummaryFileScan {
            line_count: bytes.line_count,
            first_non_empty: Self::decode_summary_line(
                path,
                "first non-empty chat line",
                bytes.first_non_empty.as_deref(),
            )?,
            last_non_empty: Self::decode_summary_line(
                path,
                "last non-empty chat line",
                bytes.last_non_empty.as_deref(),
            )?,
            fingerprint: None,
        })
    }

    async fn scan_summary_line_bytes(path: &Path) -> Result<SummaryLineByteScan, DomainError> {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!("Invalid chat backup path: {}", path.display()))
            })?;
        let (format, _) = BackupFormat::parse_physical_file_name(file_name).ok_or_else(|| {
            DomainError::InvalidData(format!("Unsupported chat backup file name: {file_name}"))
        })?;
        let mut reader = open_decoded_backup(path, format).await?;

        let mut buffer = vec![0u8; SUMMARY_SCAN_BUFFER_BYTES];
        let mut current_line = Vec::new();
        let mut scan = SummaryLineByteScan::default();

        loop {
            let bytes_read = reader.read(&mut buffer).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read chat file {:?}: {}",
                    path, error
                ))
            })?;
            if bytes_read == 0 {
                break;
            }

            for &byte in &buffer[..bytes_read] {
                if byte == b'\n' {
                    Self::finish_summary_line(path, &mut scan, &mut current_line)?;
                    continue;
                }

                current_line.push(byte);
            }
        }

        if !current_line.is_empty() {
            Self::finish_summary_line(path, &mut scan, &mut current_line)?;
        }

        Ok(scan)
    }

    fn finish_summary_line(
        path: &Path,
        scan: &mut SummaryLineByteScan,
        current_line: &mut Vec<u8>,
    ) -> Result<(), DomainError> {
        let mut line = std::mem::take(current_line);
        Self::trim_trailing_carriage_returns(&mut line);
        let is_empty = std::str::from_utf8(&line)
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to decode chat line in chat file {:?}: {}",
                    path, error
                ))
            })?
            .trim()
            .is_empty();

        if is_empty {
            line.clear();
            *current_line = line;
            return Ok(());
        }

        scan.line_count += 1;
        if scan.first_non_empty.is_none() {
            scan.first_non_empty = Some(line.clone());
        }
        if let Some(mut reusable) = scan.last_non_empty.replace(line) {
            reusable.clear();
            *current_line = reusable;
        }
        Ok(())
    }

    fn trim_trailing_carriage_returns(line: &mut Vec<u8>) {
        while line.last() == Some(&b'\r') {
            line.pop();
        }
    }

    fn decode_summary_line(
        path: &Path,
        line_name: &str,
        line: Option<&[u8]>,
    ) -> Result<Option<String>, DomainError> {
        line.map(|bytes| {
            std::str::from_utf8(bytes)
                .map(ToString::to_string)
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to decode {} in chat file {:?}: {}",
                        line_name, path, error
                    ))
                })
        })
        .transpose()
    }

    fn preview_message_text(message: &str) -> String {
        const MAX_PREVIEW_CHARS: usize = 400;

        let total_chars = message.chars().count();
        if total_chars <= MAX_PREVIEW_CHARS {
            return message.to_string();
        }

        let tail: String = message
            .chars()
            .rev()
            .take(MAX_PREVIEW_CHARS)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("...{}", tail)
    }
}

#[cfg(test)]
mod tests {
    use rand::random;
    use serde_json::json;

    use super::*;
    use crate::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;

    #[tokio::test]
    async fn backup_summary_streams_zstd_and_rejects_raw_bytes_with_zstd_suffix() {
        let root =
            std::env::temp_dir().join(format!("tauritavern-zstd-summary-{}", random::<u64>()));
        let backups_dir = root.join("backups");
        fs::create_dir_all(&backups_dir)
            .await
            .expect("create backup directory");

        let logical_file_name = "chat_alice_20260722-120000.jsonl";
        let physical_path = backups_dir.join(format!("{logical_file_name}.zst"));
        let raw_jsonl = [
            json!({
                "chat_metadata": { "chat_id_hash": 42 },
                "user_name": "User",
                "character_name": "Alice",
            })
            .to_string(),
            json!({
                "name": "User",
                "is_user": true,
                "send_date": "2026-07-21T00:00:00.000Z",
                "mes": "x".repeat(SUMMARY_SCAN_BUFFER_BYTES + 17),
                "extra": {},
            })
            .to_string(),
            json!({
                "name": "Alice",
                "is_user": false,
                "send_date": "2026-07-22T00:00:00.000Z",
                "mes": "tail response",
                "extra": {},
            })
            .to_string(),
        ]
        .join("\n");
        let compressed =
            zstd::stream::encode_all(raw_jsonl.as_bytes(), 1).expect("compress summary fixture");
        fs::write(&physical_path, &compressed)
            .await
            .expect("write zstd backup");

        let repository = FileChatRepository::with_chat_aliases(
            root.join("characters"),
            root.join("chats"),
            root.join("group chats"),
            backups_dir,
            new_shared_chat_alias_store_for_user_dir(&root),
        );
        let metadata = fs::metadata(&physical_path)
            .await
            .expect("read zstd backup metadata");
        let summary = repository
            .scan_chat_summary_file(
                &physical_path,
                "",
                logical_file_name,
                FileChatRepository::file_signature_from_metadata(&metadata),
                false,
            )
            .await
            .expect("scan zstd backup summary");

        assert_eq!(summary.summary.file_name, logical_file_name);
        assert_eq!(summary.summary.file_size, compressed.len() as u64);
        assert_eq!(summary.summary.message_count, 2);
        assert_eq!(summary.summary.preview, "tail response");

        fs::write(&physical_path, raw_jsonl)
            .await
            .expect("replace zstd backup with invalid raw bytes");
        assert!(
            repository
                .scan_chat_summary_file(
                    &physical_path,
                    "",
                    logical_file_name,
                    FileChatRepository::file_signature_from_metadata(&metadata),
                    false,
                )
                .await
                .is_err(),
            "a .jsonl.zst backup must never fall back to raw JSONL"
        );

        let _ = fs::remove_dir_all(root).await;
    }
}
