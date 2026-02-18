use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::domain::errors::DomainError;
use crate::domain::models::chat::parse_message_timestamp_value;
use crate::domain::repositories::chat_repository::ChatSearchResult;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::list_files_with_extension;

use super::FileChatRepository;

const INDEX_SCHEMA_VERSION: u32 = 1;
const FINGERPRINT_WORDS: usize = 64; // 4096 bits
const MAX_SEARCH_CACHE_ENTRIES: usize = 128;

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

    fn trigram_hashes(value: &str) -> Vec<u64> {
        let lowered = value.to_lowercase();
        let chars: Vec<char> = lowered.chars().collect();
        if chars.is_empty() {
            return Vec::new();
        }

        if chars.len() < 3 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            lowered.hash(&mut hasher);
            return vec![hasher.finish()];
        }

        let mut hashes = Vec::with_capacity(chars.len().saturating_sub(2));
        for window in chars.windows(3) {
            hashes.push(Self::hash_trigram([window[0], window[1], window[2]]));
        }
        hashes
    }

    pub(super) fn add_text(&mut self, value: &str) {
        self.normalize_len();
        for hash in Self::trigram_hashes(value) {
            self.set_hashed(hash);
        }
    }

    fn might_match_fragment(&self, fragment: &str) -> bool {
        let hashes = Self::trigram_hashes(fragment);
        if hashes.is_empty() {
            return true;
        }

        if fragment.chars().count() < 3 {
            return true;
        }

        hashes.into_iter().all(|hash| self.has_hashed(hash))
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
    pub fingerprint: SearchFingerprint,
}

#[derive(Clone)]
struct SearchCacheEntry {
    version: u64,
    results: Vec<ChatSearchResult>,
}

pub(super) struct SummaryCache {
    entries: HashMap<String, SummaryCacheEntry>,
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
}

#[derive(Serialize, Deserialize)]
struct SummaryIndexSnapshotEntry {
    key: String,
    signature: FileSignature,
    summary: ChatSearchResult,
    fingerprint: SearchFingerprint,
}

impl SummaryCache {
    pub(super) fn new(index_path: PathBuf) -> Self {
        Self {
            entries: HashMap::new(),
            search_cache: HashMap::new(),
            version: 0,
            index_path,
            loaded: false,
            dirty: false,
        }
    }

    pub(super) fn version(&self) -> u64 {
        self.version
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
                logger::warn(&format!(
                    "Failed to read chat summary index {:?}: {}",
                    self.index_path, error
                ));
                return Ok(());
            }
        };

        let snapshot: SummaryIndexSnapshot = match serde_json::from_slice(&bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                logger::warn(&format!(
                    "Failed to parse chat summary index {:?}: {}",
                    self.index_path, error
                ));
                return Ok(());
            }
        };

        if snapshot.schema_version != INDEX_SCHEMA_VERSION {
            logger::warn(&format!(
                "Skipping incompatible chat summary index schema {} (expected {})",
                snapshot.schema_version, INDEX_SCHEMA_VERSION
            ));
            return Ok(());
        }

        self.version = snapshot.version;
        for entry in snapshot.entries {
            let mut fingerprint = entry.fingerprint;
            fingerprint.normalize_len();
            self.entries.insert(
                entry.key,
                SummaryCacheEntry {
                    signature: entry.signature,
                    summary: entry.summary,
                    fingerprint,
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
        };

        serde_json::to_vec(&snapshot).map_err(|error| {
            DomainError::InternalError(format!("Failed to serialize chat summary index: {}", error))
        })
    }

    pub(super) fn get(&self, key: &str) -> Option<&SummaryCacheEntry> {
        self.entries.get(key)
    }

    pub(super) fn set(&mut self, key: String, entry: SummaryCacheEntry) {
        self.entries.insert(key, entry);
        self.bump_version();
        self.dirty = true;
    }

    pub(super) fn remove(&mut self, key: &str) {
        if self.entries.remove(key).is_some() {
            self.dirty = true;
        }
        self.bump_version();
    }

    pub(super) fn clear(&mut self) {
        if !self.entries.is_empty() {
            self.entries.clear();
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
            let key = file_stem.trim();
            if key.is_empty() {
                continue;
            }
            keys.insert(key.to_string());
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
        let (index_path, bytes, version) = {
            let mut cache = self.summary_cache.lock().await;
            cache.ensure_loaded()?;
            if !cache.is_dirty() {
                return Ok(());
            }
            (
                cache.index_path().to_path_buf(),
                cache.serialize_snapshot()?,
                cache.version(),
            )
        };

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

        let mut cache = self.summary_cache.lock().await;
        if cache.version() == version {
            cache.mark_clean();
        }

        Ok(())
    }

    pub(super) async fn list_character_chat_files(
        &self,
        character_filter: Option<&str>,
    ) -> Result<Vec<ChatFileDescriptor>, DomainError> {
        self.ensure_directory_exists().await?;

        if let Some(character_name) = character_filter {
            let dir = self.get_character_dir(character_name);
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
            let path = self.get_character_dir(&character_name);
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
                .map(|id| Self::strip_jsonl_extension(id).to_string())
                .collect();

            let mut descriptors = Vec::new();
            for id in id_set {
                let path = self.get_group_chat_path(&id);
                if !path.exists() {
                    continue;
                }
                descriptors.push(ChatFileDescriptor {
                    character_name: String::new(),
                    file_name: Self::normalize_jsonl_file_name(&id),
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
                if entry.signature == signature {
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
        let mut summary = self.get_chat_summary_entry(descriptor).await?.summary;
        if !include_metadata {
            summary.chat_metadata = None;
        }
        Ok(summary)
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

    fn file_signature_from_metadata(metadata: &std::fs::Metadata) -> FileSignature {
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
    ) -> Result<SummaryCacheEntry, DomainError> {
        let file = File::open(path).await.map_err(|error| {
            DomainError::InternalError(format!("Failed to open chat file {:?}: {}", path, error))
        })?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut line_count: usize = 0;
        let mut first_non_empty: Option<String> = None;
        let mut last_non_empty: Option<String> = None;
        let mut fingerprint = SearchFingerprint::new();
        fingerprint.add_text(Self::strip_jsonl_extension(fallback_file_name));

        while let Some(line) = lines.next_line().await.map_err(|error| {
            DomainError::InternalError(format!("Failed to read chat file {:?}: {}", path, error))
        })? {
            if line.trim().is_empty() {
                continue;
            }

            line_count += 1;
            if first_non_empty.is_none() {
                first_non_empty = Some(line.clone());
            }
            fingerprint.add_text(&line);
            last_non_empty = Some(line);
        }

        let header = first_non_empty
            .as_deref()
            .and_then(|line| serde_json::from_str::<Value>(line).ok())
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        let last_message = last_non_empty
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
        let message_count = line_count.saturating_sub(1);
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
                file_name: Self::normalize_jsonl_file_name(fallback_file_name),
                file_size: signature.size,
                message_count,
                preview,
                date,
                chat_id,
                chat_metadata: metadata,
            },
            fingerprint,
        })
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
