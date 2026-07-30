use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Local};
use tokio::fs;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::chat_directory_identity::sanitize_chat_dir_key;
use crate::file_system::unique_temp_path;
use tt_domain::errors::DomainError;
use tt_domain::models::chat::{normalize_chat_file_name, normalize_chat_file_stem};
use tt_domain::models::filename::sanitize_filename;

use super::backup_inventory::BACKUP_TEMP_PREFIX;
use super::{ContentSignature, FileChatRepository};

impl FileChatRepository {
    /// Ensure the chats directory exists
    pub(super) async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.chats_dir.exists() {
            tracing::info!("Creating chats directory: {:?}", self.chats_dir);
            fs::create_dir_all(&self.chats_dir).await.map_err(|e| {
                tracing::error!("Failed to create chats directory: {}", e);
                DomainError::InternalError(format!("Failed to create chats directory: {}", e))
            })?;
        }

        if !self.group_chats_dir.exists() {
            tracing::info!("Creating group chats directory: {:?}", self.group_chats_dir);
            fs::create_dir_all(&self.group_chats_dir)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create group chats directory: {}", e);
                    DomainError::InternalError(format!(
                        "Failed to create group chats directory: {}",
                        e
                    ))
                })?;
        }

        if !self.backups_dir.exists() {
            tracing::info!("Creating backups directory: {:?}", self.backups_dir);
            fs::create_dir_all(&self.backups_dir).await.map_err(|e| {
                tracing::error!("Failed to create backups directory: {}", e);
                DomainError::InternalError(format!("Failed to create backups directory: {}", e))
            })?;
        }

        Ok(())
    }

    pub(super) fn sanitize_path_component(value: &str, fallback: &str) -> String {
        sanitize_chat_dir_key(value, fallback)
    }

    async fn payload_lock(&self, path: &Path) -> Arc<Mutex<()>> {
        const MAX_RETAINED_LOCK_ENTRIES: usize = 2048;

        let key = path.to_path_buf();
        {
            let mut locks = self.path_write_locks.lock().await;
            if locks.len() > MAX_RETAINED_LOCK_ENTRIES {
                locks.retain(|_, value| value.strong_count() > 0);
            }

            match locks.get(&key).and_then(|value| value.upgrade()) {
                Some(existing) => existing,
                None => {
                    let created = Arc::new(Mutex::new(()));
                    locks.insert(key, Arc::downgrade(&created));
                    created
                }
            }
        }
    }

    pub(super) async fn acquire_payload_snapshot_lock(&self, path: &Path) -> OwnedMutexGuard<()> {
        self.payload_lock(path).await.lock_owned().await
    }

    pub(super) async fn try_acquire_payload_snapshot_lock(
        &self,
        path: &Path,
    ) -> Option<OwnedMutexGuard<()>> {
        self.payload_lock(path).await.try_lock_owned().ok()
    }

    pub(super) async fn acquire_payload_mutation_lock(&self, path: &Path) -> OwnedMutexGuard<()> {
        let guard = self.acquire_payload_snapshot_lock(path).await;
        self.current_content_signatures
            .lock()
            .await
            .entries
            .remove(path);
        guard
    }

    pub(super) async fn acquire_payload_rename_mutation_locks(
        &self,
        old_path: &Path,
        new_path: &Path,
    ) -> (OwnedMutexGuard<()>, Option<OwnedMutexGuard<()>>) {
        if old_path == new_path {
            return (self.acquire_payload_mutation_lock(old_path).await, None);
        }

        let guards = if old_path < new_path {
            let old_guard = self.acquire_payload_snapshot_lock(old_path).await;
            let new_guard = self.acquire_payload_snapshot_lock(new_path).await;
            (old_guard, Some(new_guard))
        } else {
            let new_guard = self.acquire_payload_snapshot_lock(new_path).await;
            let old_guard = self.acquire_payload_snapshot_lock(old_path).await;
            (old_guard, Some(new_guard))
        };

        let mut signatures = self.current_content_signatures.lock().await;
        signatures.entries.remove(old_path);
        signatures.entries.remove(new_path);
        drop(signatures);
        guards
    }

    pub(super) async fn current_content_signature_epoch(&self) -> u64 {
        self.current_content_signatures.lock().await.epoch
    }

    pub(super) async fn record_current_content_signature(
        &self,
        path: &Path,
        expected_epoch: u64,
        signature: ContentSignature,
    ) {
        let mut signatures = self.current_content_signatures.lock().await;
        if signatures.epoch == expected_epoch {
            signatures.entries.insert(path.to_path_buf(), signature);
        }
    }

    pub(super) async fn current_content_signature_for_size(
        &self,
        path: &Path,
        byte_len: u64,
    ) -> Option<ContentSignature> {
        let mut signatures = self.current_content_signatures.lock().await;
        let signature = signatures.entries.get(path).copied()?;
        if signature.byte_len == byte_len {
            return Some(signature);
        }

        signatures.entries.remove(path);
        tracing::error!(
            source = %path.display(),
            cached_bytes = signature.byte_len,
            actual_bytes = byte_len,
            "Discarded stale current content signature"
        );
        None
    }

    /// Invalidates path-bound signatures after whole-directory or external data changes.
    pub async fn invalidate_all_payload_signatures(&self) {
        self.current_content_signatures
            .lock()
            .await
            .invalidate_all();
    }

    pub(super) fn temp_payload_path(path: &Path) -> PathBuf {
        unique_temp_path(path)
    }

    pub(super) fn normalize_jsonl_file_stem(file_name: &str) -> Result<String, DomainError> {
        normalize_chat_file_stem(file_name)
            .ok_or_else(|| DomainError::InvalidData("Invalid chat file name".to_string()))
    }

    pub(super) fn get_character_dir_for_key(&self, dir_key: &str) -> PathBuf {
        self.chats_dir.join(dir_key)
    }

    /// Normalize chat file names with SillyTavern-compatible `.jsonl` sanitization.
    pub(super) fn normalize_jsonl_file_name(file_name: &str) -> Result<String, DomainError> {
        normalize_chat_file_name(file_name)
            .ok_or_else(|| DomainError::InvalidData("Invalid chat file name".to_string()))
    }

    /// Build a timestamp that is safe to use in file names on all platforms.
    fn backup_timestamp(at: DateTime<Local>) -> String {
        at.format("%Y%m%d-%H%M%S").to_string()
    }

    /// Keeps SillyTavern's readable backup key while preserving Unicode letters and numbers.
    pub(super) fn sanitize_backup_name_for_sillytavern(input: &str) -> String {
        let mut sanitized = String::with_capacity(input.len());

        for ch in input.chars() {
            let is_invalid = matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || ch.is_control();
            if !is_invalid {
                sanitized.push(ch);
            }
        }

        let trimmed = sanitized.trim_matches([' ', '.']).to_string();
        let lowered = trimmed.to_ascii_lowercase();

        let is_reserved = matches!(
            lowered.as_str(),
            "" | "."
                | ".."
                | "con"
                | "prn"
                | "aux"
                | "nul"
                | "com1"
                | "com2"
                | "com3"
                | "com4"
                | "com5"
                | "com6"
                | "com7"
                | "com8"
                | "com9"
                | "lpt1"
                | "lpt2"
                | "lpt3"
                | "lpt4"
                | "lpt5"
                | "lpt6"
                | "lpt7"
                | "lpt8"
                | "lpt9"
        );

        if is_reserved {
            return String::new();
        }

        const FINAL_NAME_FIXED_BYTES: usize =
            "chat_".len() + "_".len() + "YYYYMMDD-HHMMSS".len() + ".jsonl.zst".len();
        const MAX_SANITIZED_BYTES: usize = 255 - FINAL_NAME_FIXED_BYTES;

        let mut result = String::with_capacity(lowered.len().min(MAX_SANITIZED_BYTES));
        for ch in lowered.chars() {
            let output = if ch.is_alphanumeric() { ch } else { '_' };
            if result.len() + output.len_utf8() > MAX_SANITIZED_BYTES {
                break;
            }
            result.push(output);
        }
        result
    }

    pub(super) fn backup_file_prefix(character_name: &str) -> String {
        format!(
            "{}{}_",
            Self::CHAT_BACKUP_PREFIX,
            Self::sanitize_backup_name_for_sillytavern(character_name)
        )
    }

    /// Build backup file name in the form `chat_<sanitized_character>_<timestamp>.jsonl`.
    pub(super) fn backup_file_name_at(character_name: &str, at: DateTime<Local>) -> String {
        format!(
            "{}{}.jsonl",
            Self::backup_file_prefix(character_name),
            Self::backup_timestamp(at)
        )
    }

    #[cfg(test)]
    pub(super) fn backup_file_name(character_name: &str) -> String {
        Self::backup_file_name_at(character_name, Local::now())
    }

    pub(super) fn backup_temp_path(&self) -> PathBuf {
        self.backups_dir.join(format!(
            "{}{}",
            BACKUP_TEMP_PREFIX,
            uuid::Uuid::new_v4().simple()
        ))
    }

    /// Get the path to a group chat file
    pub(super) fn get_group_chat_path(&self, chat_id: &str) -> Result<PathBuf, DomainError> {
        let normalized = Self::normalize_jsonl_file_name(chat_id)?;
        Ok(self.group_chats_dir.join(normalized))
    }

    pub(super) fn normalize_backup_file_name(
        backup_file_name: &str,
    ) -> Result<String, DomainError> {
        let trimmed = backup_file_name.trim();
        if trimmed.is_empty() {
            return Err(DomainError::InvalidData(
                "Backup file name cannot be empty".to_string(),
            ));
        }

        let leaf_name = std::path::Path::new(trimmed)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| DomainError::InvalidData("Invalid backup file name".to_string()))?;

        let sanitized = sanitize_filename(leaf_name).trim().to_string();
        if sanitized.is_empty() {
            return Err(DomainError::InvalidData(
                "Invalid backup file name".to_string(),
            ));
        }

        if !sanitized.starts_with(Self::CHAT_BACKUP_PREFIX) || !sanitized.ends_with(".jsonl") {
            return Err(DomainError::InvalidData(
                "Invalid chat backup file name".to_string(),
            ));
        }

        Ok(sanitized)
    }

    /// Get the cache key for a chat
    pub(super) fn get_cache_key(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<String, DomainError> {
        Ok(format!(
            "{}:{}",
            Self::sanitize_path_component(character_name, "character"),
            Self::normalize_jsonl_file_stem(file_name)?
        ))
    }
}
