use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_repository::{ChatBackupCatalogEntry, ChatSearchResult};

use crate::file_system::write_json_file;

use super::FileChatRepository;
use super::backup_inventory::{BackupEntry, BackupInventory};
use super::summary::FileSignature;

const INDEX_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct BackupSummarySignature {
    physical_file_name: String,
    stored_size: u64,
    modified_millis: i64,
}

impl BackupSummarySignature {
    fn from_entry(entry: &BackupEntry) -> Self {
        Self {
            physical_file_name: entry.file_name.clone(),
            stored_size: entry.byte_len,
            modified_millis: backup_modified_millis(entry),
        }
    }

    fn matches(&self, entry: &BackupEntry) -> bool {
        self.physical_file_name == entry.file_name
            && self.stored_size == entry.byte_len
            && self.modified_millis == backup_modified_millis(entry)
    }
}

fn backup_modified_millis(entry: &BackupEntry) -> i64 {
    entry
        .modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BackupSummaryCacheEntry {
    signature: BackupSummarySignature,
    jsonl_record_count: usize,
    #[serde(skip)]
    full_summary: Option<ChatSearchResult>,
}

#[derive(Serialize, Deserialize)]
struct BackupSummaryIndexSnapshot {
    schema_version: u32,
    entries: HashMap<String, BackupSummaryCacheEntry>,
}

pub(super) struct BackupSummaryCache {
    entries: HashMap<String, BackupSummaryCacheEntry>,
    index_path: PathBuf,
    loaded: bool,
    dirty: bool,
}

impl BackupSummaryCache {
    pub(super) fn new(index_path: PathBuf) -> Self {
        Self {
            entries: HashMap::new(),
            index_path,
            loaded: false,
            dirty: false,
        }
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;

        let bytes = match std::fs::read(&self.index_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                tracing::warn!(
                    path = %self.index_path.display(),
                    %error,
                    "Failed to read chat backup summary index"
                );
                return;
            }
        };
        let snapshot: BackupSummaryIndexSnapshot = match serde_json::from_slice(&bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    path = %self.index_path.display(),
                    %error,
                    "Failed to parse chat backup summary index"
                );
                return;
            }
        };
        if snapshot.schema_version != INDEX_SCHEMA_VERSION {
            tracing::warn!(
                schema_version = snapshot.schema_version,
                expected = INDEX_SCHEMA_VERSION,
                "Skipping incompatible chat backup summary index"
            );
            return;
        }

        self.entries = snapshot.entries;
    }

    fn matching_entry(&self, entry: &BackupEntry) -> Option<&BackupSummaryCacheEntry> {
        self.entries
            .get(&entry.logical_file_name)
            .filter(|cached| cached.signature.matches(entry))
    }

    fn message_count(&self, entry: &BackupEntry) -> Option<usize> {
        self.matching_entry(entry)
            .map(|cached| cached.jsonl_record_count.saturating_sub(1))
    }

    fn summary(&self, entry: &BackupEntry) -> Option<ChatSearchResult> {
        self.matching_entry(entry)
            .and_then(|cached| cached.full_summary.clone())
    }

    fn record_count(&mut self, entry: &BackupEntry, jsonl_record_count: usize) {
        self.entries.insert(
            entry.logical_file_name.clone(),
            BackupSummaryCacheEntry {
                signature: BackupSummarySignature::from_entry(entry),
                jsonl_record_count,
                full_summary: None,
            },
        );
        self.dirty = true;
    }

    fn record_summary(&mut self, entry: &BackupEntry, summary: ChatSearchResult) {
        let signature = BackupSummarySignature::from_entry(entry);
        let (jsonl_record_count, persistent_changed) =
            match self.entries.get(&entry.logical_file_name) {
                Some(cached) if cached.signature == signature => (cached.jsonl_record_count, false),
                _ => (summary.message_count.saturating_add(1), true),
            };
        self.entries.insert(
            entry.logical_file_name.clone(),
            BackupSummaryCacheEntry {
                signature,
                jsonl_record_count,
                full_summary: Some(summary),
            },
        );
        self.dirty |= persistent_changed;
    }

    fn update_signature(&mut self, entry: &BackupEntry) {
        let Some(cached) = self.entries.get_mut(&entry.logical_file_name) else {
            return;
        };

        cached.signature = BackupSummarySignature::from_entry(entry);
        if let Some(summary) = cached.full_summary.as_mut() {
            summary.file_name = entry.logical_file_name.clone();
            summary.file_size = entry.byte_len;
        }
        self.dirty = true;
    }

    fn remove(&mut self, logical_file_name: &str) {
        if self.entries.remove(logical_file_name).is_some() {
            self.dirty = true;
        }
    }

    fn reconcile(&mut self, inventory: &BackupInventory) {
        let signatures: HashMap<_, _> = inventory
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.logical_file_name.as_str(),
                    BackupSummarySignature::from_entry(entry),
                )
            })
            .collect();
        let before = self.entries.len();
        self.entries.retain(|logical_file_name, cached| {
            signatures.get(logical_file_name.as_str()) == Some(&cached.signature)
        });
        self.dirty |= self.entries.len() != before;
    }

    fn snapshot(&self) -> BackupSummaryIndexSnapshot {
        BackupSummaryIndexSnapshot {
            schema_version: INDEX_SCHEMA_VERSION,
            entries: self.entries.clone(),
        }
    }
}

impl FileChatRepository {
    pub(super) async fn list_chat_backup_catalog_entries(
        &self,
    ) -> Result<Vec<ChatBackupCatalogEntry>, DomainError> {
        let entries = self.list_chat_backup_entries().await?;
        let mut cache = self.backup_summary_cache.lock().await;
        cache.ensure_loaded();
        let catalog = entries
            .iter()
            .map(|entry| ChatBackupCatalogEntry {
                file_name: entry.logical_file_name.clone(),
                stored_size: entry.byte_len,
                backup_date: backup_modified_millis(entry),
                message_count: cache.message_count(entry),
            })
            .collect();
        Ok(catalog)
    }

    pub(super) async fn get_chat_backup_summary(
        &self,
        entry: &BackupEntry,
    ) -> Result<ChatSearchResult, DomainError> {
        {
            let mut cache = self.backup_summary_cache.lock().await;
            cache.ensure_loaded();
            if let Some(summary) = cache.summary(entry) {
                return Ok(summary);
            }
        }

        let signature = BackupSummarySignature::from_entry(entry);
        let scanned = self
            .scan_chat_summary_file(
                &self.backups_dir.join(&entry.file_name),
                "",
                &entry.logical_file_name,
                FileSignature {
                    size: signature.stored_size,
                    modified_millis: signature.modified_millis,
                },
                false,
            )
            .await?;
        let summary = scanned.summary;

        let mut cache = self.backup_summary_cache.lock().await;
        cache.ensure_loaded();
        cache.record_summary(entry, summary.clone());
        Ok(summary)
    }

    pub(super) async fn record_backup_jsonl_count(
        &self,
        entry: &BackupEntry,
        jsonl_record_count: usize,
    ) {
        let mut cache = self.backup_summary_cache.lock().await;
        cache.ensure_loaded();
        cache.record_count(entry, jsonl_record_count);
    }

    pub(super) async fn update_backup_summary_signature(&self, entry: &BackupEntry) {
        let mut cache = self.backup_summary_cache.lock().await;
        cache.ensure_loaded();
        cache.update_signature(entry);
    }

    pub(super) async fn remove_backup_summary(&self, logical_file_name: &str) {
        let mut cache = self.backup_summary_cache.lock().await;
        cache.ensure_loaded();
        cache.remove(logical_file_name);
    }

    pub(super) async fn reconcile_backup_summary_index(&self, inventory: &BackupInventory) {
        let mut cache = self.backup_summary_cache.lock().await;
        cache.ensure_loaded();
        cache.reconcile(inventory);
    }

    pub(super) fn schedule_backup_summary_index_flush(&self) {
        let cache = Arc::clone(&self.backup_summary_cache);
        tokio::spawn(async move {
            if let Err(error) = Self::flush_backup_summary_cache(&cache).await {
                tracing::warn!(%error, "Failed to persist chat backup summary index");
            }
        });
    }

    async fn flush_backup_summary_cache(
        cache: &Arc<Mutex<BackupSummaryCache>>,
    ) -> Result<(), DomainError> {
        let mut cache = cache.lock().await;
        cache.ensure_loaded();
        if !cache.dirty {
            return Ok(());
        }

        let index_path = cache.index_path.clone();
        write_json_file(&index_path, &cache.snapshot()).await?;
        cache.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::super::backup_codec::BackupFormat;
    use super::*;

    fn entry(name: &str) -> BackupEntry {
        BackupEntry {
            logical_file_name: name.to_string(),
            file_name: format!("{name}.zst"),
            format: BackupFormat::Zstd,
            parsed_prefix: None,
            modified: UNIX_EPOCH + Duration::from_secs(1),
            byte_len: 42,
            content_signature: None,
        }
    }

    #[test]
    fn backup_count_is_used_only_for_the_matching_physical_file() {
        let mut cache = BackupSummaryCache::new(PathBuf::from("unused"));
        cache.loaded = true;
        let original = entry("chat_alice_20260101-000000.jsonl");
        cache.record_count(&original, 4);
        assert_eq!(cache.message_count(&original), Some(3));

        let mut replaced = original.clone();
        replaced.byte_len += 1;
        assert_eq!(cache.message_count(&replaced), None);
    }

    #[test]
    fn in_memory_full_summary_does_not_dirty_unchanged_index_data() {
        let mut cache = BackupSummaryCache::new(PathBuf::from("unused"));
        cache.loaded = true;
        let entry = entry("chat_alice_20260101-000000.jsonl");
        cache.record_count(&entry, 4);
        cache.dirty = false;

        cache.record_summary(
            &entry,
            ChatSearchResult {
                character_name: String::new(),
                file_name: entry.logical_file_name.clone(),
                file_size: entry.byte_len,
                message_count: 3,
                preview: "tail".to_string(),
                date: 1,
                chat_id: None,
                chat_metadata: None,
            },
        );

        assert!(!cache.dirty);
        assert_eq!(cache.summary(&entry).unwrap().preview, "tail");
    }
}
