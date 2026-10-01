use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;
use tt_domain::errors::DomainError;

use crate::file_system::write_json_file;

use super::super::FileChatRepository;
use super::search::SearchFingerprint;
use super::{ChatSummary, FileSignature, summary_cache_key};

// The file name isolates the old metadata-heavy cache; schema versions invalidate
// changes to persisted fields OR projection semantics (preview, dates, fingerprints).
const INDEX_FILE_NAME: &str = "chat_summary_index_v2.json";
const LEGACY_INDEX_FILE_NAMES: &[&str] = &["chat_summary_index_v1.json"];
pub(super) const SCHEMA_VERSION: u32 = 1;

const MAX_SEARCH_ENTRIES: usize = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::repositories::file_chat_repository) struct SummaryCacheEntry {
    pub(super) signature: FileSignature,
    pub(in crate::repositories::file_chat_repository) summary: ChatSummary,
    pub(super) preview_unavailable: bool,
    pub(super) fingerprint: Option<SearchFingerprint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ChatStatsCacheEntry {
    pub(super) signature: FileSignature,
    pub(super) date: i64,
}

pub(in crate::repositories::file_chat_repository) struct SummaryCache {
    entries: HashMap<String, SummaryCacheEntry>,
    stats_entries: HashMap<String, ChatStatsCacheEntry>,
    search_cache: HashMap<String, Vec<ChatSummary>>,
    index_path: PathBuf,
    loaded: bool,
    dirty: bool,
}

// Cow reads owned maps and writes borrowed maps without cloning the runtime cache.
#[derive(Serialize, Deserialize)]
struct Snapshot<'a> {
    schema_version: u32,
    entries: Cow<'a, HashMap<String, SummaryCacheEntry>>,
    stats_entries: Cow<'a, HashMap<String, ChatStatsCacheEntry>>,
}

impl SummaryCache {
    pub(in crate::repositories::file_chat_repository) fn new(cache_dir: PathBuf) -> Self {
        Self {
            entries: HashMap::new(),
            stats_entries: HashMap::new(),
            search_cache: HashMap::new(),
            index_path: cache_dir.join(INDEX_FILE_NAME),
            loaded: false,
            dirty: false,
        }
    }

    pub(super) async fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        // Obsolete caches are never read. Cleanup is independent of publishing a
        // new snapshot, and failures must not block a directory query.
        for name in LEGACY_INDEX_FILE_NAMES {
            let path = self.index_path.with_file_name(name);
            if let Err(error) = fs::remove_file(&path).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(path = %path.display(), %error, "Failed to remove obsolete chat summary index");
            }
        }
        let bytes = match fs::read(&self.index_path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                tracing::warn!(path = %self.index_path.display(), %error, "Failed to read chat summary index; rebuilding from scans");
                return;
            }
        };
        let snapshot: Snapshot<'_> = match serde_json::from_slice(&bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(path = %self.index_path.display(), %error, "Failed to parse chat summary index; rebuilding from scans");
                return;
            }
        };
        if snapshot.schema_version != SCHEMA_VERSION {
            tracing::warn!(
                schema_version = snapshot.schema_version,
                expected = SCHEMA_VERSION,
                "Skipping incompatible chat summary index; rebuilding from scans"
            );
            return;
        }
        self.entries = snapshot.entries.into_owned();
        self.stats_entries = snapshot.stats_entries.into_owned();
        for entry in self.entries.values_mut() {
            if let Some(fingerprint) = entry.fingerprint.as_mut() {
                fingerprint.normalize_len();
            }
        }
    }

    pub(super) fn get(&self, key: &str) -> Option<&SummaryCacheEntry> {
        self.entries.get(key)
    }

    pub(super) fn set(&mut self, key: String, entry: SummaryCacheEntry) {
        self.stats_entries.remove(&key);
        self.entries.insert(key, entry);
        self.search_cache.clear();
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
        self.search_cache.clear();
        self.dirty = true;
    }

    pub(super) fn remove(&mut self, key: &str) {
        let removed_summary = self.entries.remove(key).is_some();
        let removed_stats = self.stats_entries.remove(key).is_some();
        if removed_summary || removed_stats {
            self.dirty = true;
        }
        self.search_cache.clear();
    }

    pub(super) fn clear(&mut self) {
        if !self.entries.is_empty() || !self.stats_entries.is_empty() {
            self.entries.clear();
            self.stats_entries.clear();
            self.dirty = true;
        }
        self.search_cache.clear();
    }

    pub(super) fn get_search_results(&self, key: &str) -> Option<Vec<ChatSummary>> {
        self.search_cache.get(key).cloned()
    }

    pub(super) fn set_search_results(&mut self, key: String, results: Vec<ChatSummary>) {
        if self.search_cache.len() >= MAX_SEARCH_ENTRIES {
            self.search_cache.clear();
        }
        self.search_cache.insert(key, results);
    }
}

impl FileChatRepository {
    pub(in crate::repositories::file_chat_repository) async fn clear_summary_cache(&self) {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded().await;
        cache.clear();
    }

    pub async fn clear_chat_summary_index(&self) {
        {
            let mut cache = self.summary_cache.lock().await;
            cache.ensure_loaded().await;
            cache.clear();
        }
        self.flush_summary_index_best_effort().await;
    }

    pub(in crate::repositories::file_chat_repository) async fn remove_summary_cache_for_path(
        &self,
        path: &Path,
    ) {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded().await;
        cache.remove(&summary_cache_key(path));
    }

    pub(in crate::repositories::file_chat_repository) async fn get_cached_search_results(
        &self,
        key: &str,
    ) -> Option<Vec<ChatSummary>> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded().await;
        cache.get_search_results(key)
    }

    pub(in crate::repositories::file_chat_repository) async fn cache_search_results(
        &self,
        key: String,
        results: Vec<ChatSummary>,
    ) {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded().await;
        cache.set_search_results(key, results);
    }

    pub(in crate::repositories::file_chat_repository) async fn flush_summary_index_if_needed(
        &self,
    ) -> Result<(), DomainError> {
        let mut cache = self.summary_cache.lock().await;
        cache.ensure_loaded().await;
        if !cache.dirty {
            return Ok(());
        }
        write_json_file(
            &cache.index_path,
            &Snapshot {
                schema_version: SCHEMA_VERSION,
                entries: Cow::Borrowed(&cache.entries),
                stats_entries: Cow::Borrowed(&cache.stats_entries),
            },
        )
        .await?;
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
