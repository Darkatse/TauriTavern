use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tauri::{AppHandle, Emitter};

use super::LLM_API_LOG_EVENT;
use super::files::{
    delete_entry_files, index_path, load_meta, meta_path, persist_index_file, persist_meta_file,
    persist_raw_files, request_raw_path, response_raw_json_path, response_raw_sse_path,
};
use super::types::{
    LlmApiLogEntryPreview, LlmApiLogEntryRaw, LlmApiLogIndexEntry, LlmApiLogMeta, LlmApiRawKind,
};

const DEFAULT_KEEP: usize = 5;

pub struct LlmApiLogStore {
    app_handle: AppHandle,
    log_root: PathBuf,
    next_id: AtomicU64,
    stream_enabled: AtomicBool,
    keep: AtomicU64,
    index: Mutex<VecDeque<LlmApiLogIndexEntry>>,
}

impl LlmApiLogStore {
    pub fn new(app_handle: AppHandle, log_root: PathBuf) -> Self {
        let mut index = VecDeque::new();
        let mut next_id = 1_u64;

        if let Ok(content) = std::fs::read_to_string(index_path(&log_root)) {
            if let Ok(entries) = serde_json::from_str::<Vec<LlmApiLogIndexEntry>>(&content) {
                for entry in entries {
                    next_id = next_id.max(entry.id.saturating_add(1));
                    index.push_back(entry);
                }
            }
        }

        Self {
            app_handle,
            log_root,
            next_id: AtomicU64::new(next_id),
            stream_enabled: AtomicBool::new(false),
            keep: AtomicU64::new(DEFAULT_KEEP as u64),
            index: Mutex::new(index),
        }
    }

    pub(super) fn log_root(&self) -> &Path {
        &self.log_root
    }

    pub fn set_stream_enabled(&self, enabled: bool) {
        self.stream_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn apply_settings(&self, keep: u32) {
        let keep = keep as usize;
        self.keep.store(keep as u64, Ordering::Relaxed);
        self.enforce_keep_limit();

        let index_snapshot = {
            let index = self.index.lock().unwrap();
            index.iter().cloned().collect::<Vec<_>>()
        };
        let log_root = self.log_root.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = persist_index_file(&log_root, &index_snapshot).await {
                tracing::error!("Failed to persist LLM API log index: {}", error);
            }
        });
    }

    pub(super) fn allocate_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn tail_index(&self, limit: usize) -> Vec<LlmApiLogIndexEntry> {
        let entries = self.index.lock().unwrap();
        let len = entries.len();
        let start = len.saturating_sub(limit);
        entries.iter().skip(start).cloned().collect()
    }

    pub async fn get_preview(&self, id: u64) -> Result<LlmApiLogEntryPreview, std::io::Error> {
        Ok(load_meta(meta_path(&self.log_root, id)).await?.into())
    }

    pub async fn get_raw(&self, id: u64) -> Result<LlmApiLogEntryRaw, std::io::Error> {
        let meta = load_meta(meta_path(&self.log_root, id)).await?;
        let request_raw = tokio::fs::read_to_string(request_raw_path(&self.log_root, id)).await?;

        let response_raw = match meta.response_raw_kind {
            Some(LlmApiRawKind::Json) => {
                tokio::fs::read_to_string(response_raw_json_path(&self.log_root, id)).await?
            }
            Some(LlmApiRawKind::Sse) => {
                tokio::fs::read_to_string(response_raw_sse_path(&self.log_root, id)).await?
            }
            None => String::new(),
        };

        Ok(LlmApiLogEntryRaw {
            id,
            request_raw,
            response_raw,
            response_raw_kind: meta.response_raw_kind,
        })
    }

    pub(super) async fn record_entry(
        &self,
        meta: LlmApiLogMeta,
        request_raw_inline: Option<String>,
        response_raw_inline: Option<String>,
    ) {
        if let Err(error) = persist_meta_file(&self.log_root, &meta).await {
            tracing::error!(
                "Failed to persist LLM API meta entry {}: {}",
                meta.id,
                error
            );
            return;
        }

        let index_entry = LlmApiLogIndexEntry::from(&meta);
        let keep = self.keep.load(Ordering::Relaxed) as usize;
        let should_stream = self.stream_enabled.load(Ordering::Relaxed);

        let (removed_ids, index_snapshot) = {
            let mut index = self.index.lock().unwrap();
            index.push_back(index_entry.clone());
            let mut removed = Vec::new();
            while index.len() > keep {
                if let Some(entry) = index.pop_front() {
                    removed.push(entry.id);
                }
            }
            (removed, index.iter().cloned().collect::<Vec<_>>())
        };

        if should_stream {
            let _ = self.app_handle.emit(LLM_API_LOG_EVENT, index_entry);
        }

        let log_root = self.log_root.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = persist_raw_files(
                &log_root,
                meta.id,
                request_raw_inline.as_deref(),
                meta.response_raw_kind,
                response_raw_inline.as_deref(),
            )
            .await
            {
                tracing::error!(
                    "Failed to persist LLM API log raw entry {}: {}",
                    meta.id,
                    error
                );
            }

            for removed_id in removed_ids {
                if let Err(error) = delete_entry_files(&log_root, removed_id).await {
                    tracing::warn!(
                        "Failed to delete old LLM API log entry {}: {}",
                        removed_id,
                        error
                    );
                }
            }

            if let Err(error) = persist_index_file(&log_root, &index_snapshot).await {
                tracing::error!("Failed to persist LLM API log index: {}", error);
            }
        });
    }

    fn enforce_keep_limit(&self) {
        let keep = self.keep.load(Ordering::Relaxed) as usize;
        let removed_ids = {
            let mut index = self.index.lock().unwrap();
            let mut removed = Vec::new();
            while index.len() > keep {
                if let Some(entry) = index.pop_front() {
                    removed.push(entry.id);
                }
            }
            removed
        };

        if removed_ids.is_empty() {
            return;
        }

        let log_root = self.log_root.clone();
        tauri::async_runtime::spawn(async move {
            for removed_id in removed_ids {
                let _ = delete_entry_files(&log_root, removed_id).await;
            }
        });
    }
}
