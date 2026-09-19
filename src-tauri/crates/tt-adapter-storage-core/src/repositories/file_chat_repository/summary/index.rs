use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_repository::ChatSearchResult;

use super::super::FileChatRepository;
use super::search::SearchFingerprint;
use super::{FileSignature, summary_cache_key};

const SCHEMA_VERSION: u32 = 1;
const MAX_SEARCH_ENTRIES: usize = 128;

#[derive(Clone, Debug)]
pub(in crate::repositories::file_chat_repository) struct SummaryCacheEntry {
    pub(super) signature: FileSignature,
    pub(in crate::repositories::file_chat_repository) summary: ChatSearchResult,
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

#[derive(Serialize, Deserialize)]
struct Snapshot {
    schema_version: u32,
    version: u64,
    entries: Vec<SnapshotEntry>,
    #[serde(default)]
    stats_entries: Vec<StatsSnapshotEntry>,
}

#[derive(Serialize, Deserialize)]
struct SnapshotEntry {
    key: String,
    signature: FileSignature,
    summary: ChatSearchResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fingerprint: Option<SearchFingerprint>,
}

#[derive(Serialize, Deserialize)]
struct StatsSnapshotEntry {
    key: String,
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
                tracing::warn!(path = %self.index_path.display(), %error, "Failed to read chat summary index");
                return Ok(());
            }
        };
        let mut snapshot: Snapshot = match serde_json::from_slice(&bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(path = %self.index_path.display(), %error, "Failed to parse chat summary index");
                return Ok(());
            }
        };
        if snapshot.schema_version != SCHEMA_VERSION {
            tracing::warn!(
                schema_version = snapshot.schema_version,
                expected = SCHEMA_VERSION,
                "Skipping incompatible chat summary index"
            );
            return Ok(());
        }

        let previous_len = snapshot.entries.len() + snapshot.stats_entries.len();
        snapshot
            .entries
            .retain(|entry| !Path::new(&entry.key).starts_with(&self.backups_dir));
        snapshot
            .stats_entries
            .retain(|entry| !Path::new(&entry.key).starts_with(&self.backups_dir));
        self.dirty = snapshot.entries.len() + snapshot.stats_entries.len() != previous_len;
        self.version = snapshot.version;

        self.entries = snapshot
            .entries
            .into_iter()
            .map(|entry| {
                let mut fingerprint = entry.fingerprint;
                if let Some(value) = fingerprint.as_mut() {
                    value.normalize_len();
                }
                (
                    entry.key,
                    SummaryCacheEntry {
                        signature: entry.signature,
                        summary: entry.summary,
                        fingerprint,
                    },
                )
            })
            .collect();
        self.stats_entries = snapshot
            .stats_entries
            .into_iter()
            .map(|entry| {
                (
                    entry.key,
                    ChatStatsCacheEntry {
                        signature: entry.signature,
                        date: entry.date,
                    },
                )
            })
            .collect();
        Ok(())
    }

    fn snapshot_bytes(&self) -> Result<Vec<u8>, DomainError> {
        serde_json::to_vec(&Snapshot {
            schema_version: SCHEMA_VERSION,
            version: self.version,
            entries: self
                .entries
                .iter()
                .map(|(key, entry)| SnapshotEntry {
                    key: key.clone(),
                    signature: entry.signature,
                    summary: entry.summary.clone(),
                    fingerprint: entry.fingerprint.clone(),
                })
                .collect(),
            stats_entries: self
                .stats_entries
                .iter()
                .map(|(key, entry)| StatsSnapshotEntry {
                    key: key.clone(),
                    signature: entry.signature,
                    date: entry.date,
                })
                .collect(),
        })
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to serialize chat summary index: {error}"))
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
        if cache.ensure_loaded().is_ok() {
            cache.clear();
        }
    }

    pub async fn clear_chat_summary_index(&self) {
        {
            let mut cache = self.summary_cache.lock().await;
            if cache.ensure_loaded().is_err() {
                return;
            }
            cache.clear();
        }
        self.flush_summary_index_best_effort().await;
    }

    pub(in crate::repositories::file_chat_repository) async fn remove_summary_cache_for_path(
        &self,
        path: &Path,
    ) {
        let mut cache = self.summary_cache.lock().await;
        if cache.ensure_loaded().is_ok() {
            cache.remove(&summary_cache_key(path));
        }
    }

    pub(in crate::repositories::file_chat_repository) async fn get_cached_search_results(
        &self,
        key: &str,
    ) -> Option<Vec<ChatSearchResult>> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded().ok()?;
        cache.get_search_results(key)
    }

    pub(in crate::repositories::file_chat_repository) async fn cache_search_results(
        &self,
        key: String,
        results: Vec<ChatSearchResult>,
    ) {
        let mut cache = self.summary_cache.lock().await;
        if cache.ensure_loaded().is_ok() {
            cache.set_search_results(key, results);
        }
    }

    pub(in crate::repositories::file_chat_repository) async fn flush_summary_index_if_needed(
        &self,
    ) -> Result<(), DomainError> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded()?;
        if !cache.dirty {
            return Ok(());
        }

        let index_path = cache.index_path.clone();
        let bytes = cache.snapshot_bytes()?;
        if let Some(parent) = index_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create chat summary index directory {:?}: {error}",
                    parent
                ))
            })?;
        }
        fs::write(&index_path, bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write chat summary index {:?}: {error}",
                index_path
            ))
        })?;
        cache.dirty = false;
        Ok(())
    }

    pub(in crate::repositories::file_chat_repository) async fn flush_summary_index_best_effort(
        &self,
    ) {
        if let Err(error) = self.flush_summary_index_if_needed().await {
            tracing::warn!(%error, "Failed to persist chat summary index");
        }
    }
}
