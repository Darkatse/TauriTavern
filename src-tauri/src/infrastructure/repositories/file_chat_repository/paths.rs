use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Local;
use tokio::fs;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::infrastructure::repositories::chat_directory_identity::sanitize_chat_dir_key;
use tt_adapter_storage_core::file_system::unique_temp_path;
use tt_domain::errors::DomainError;
use tt_domain::models::chat::{normalize_chat_file_name, normalize_chat_file_stem};
use tt_domain::models::filename::sanitize_filename;

use super::FileChatRepository;

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

    pub(super) async fn acquire_payload_write_lock(&self, path: &Path) -> OwnedMutexGuard<()> {
        const MAX_RETAINED_LOCK_ENTRIES: usize = 2048;

        let key = path.to_path_buf();
        let lock = {
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
        };

        lock.lock_owned().await
    }

    pub(super) async fn acquire_payload_rename_locks(
        &self,
        old_path: &Path,
        new_path: &Path,
    ) -> (OwnedMutexGuard<()>, Option<OwnedMutexGuard<()>>) {
        if old_path == new_path {
            return (self.acquire_payload_write_lock(old_path).await, None);
        }

        if old_path < new_path {
            let old_guard = self.acquire_payload_write_lock(old_path).await;
            let new_guard = self.acquire_payload_write_lock(new_path).await;
            (old_guard, Some(new_guard))
        } else {
            let new_guard = self.acquire_payload_write_lock(new_path).await;
            let old_guard = self.acquire_payload_write_lock(old_path).await;
            (old_guard, Some(new_guard))
        }
    }

    pub(super) fn temp_payload_path(path: &Path) -> PathBuf {
        unique_temp_path(path, "chat.jsonl")
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
    fn backup_timestamp() -> String {
        Local::now().format("%Y%m%d-%H%M%S").to_string()
    }

    /// Mirrors SillyTavern backup name normalization:
    /// sanitize(name).replace(/[^a-z0-9]/gi, '_').toLowerCase()
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

        lowered
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect()
    }

    pub(super) fn backup_file_prefix(character_name: &str) -> String {
        format!(
            "{}{}_",
            Self::CHAT_BACKUP_PREFIX,
            Self::sanitize_backup_name_for_sillytavern(character_name)
        )
    }

    /// Build backup file name in the form `chat_<sanitized_character>_<timestamp>.jsonl`.
    pub(super) fn backup_file_name(character_name: &str) -> String {
        format!(
            "{}{}.jsonl",
            Self::backup_file_prefix(character_name),
            Self::backup_timestamp()
        )
    }

    /// Get the path to a group chat file
    pub(super) fn get_group_chat_path(&self, chat_id: &str) -> Result<PathBuf, DomainError> {
        let normalized = Self::normalize_jsonl_file_name(chat_id)?;
        Ok(self.group_chats_dir.join(normalized))
    }

    /// Get the path to a chat backup file
    pub(super) fn get_backup_path(&self, backup_name: &str) -> PathBuf {
        self.backups_dir.join(Self::backup_file_name(backup_name))
    }

    pub(super) fn resolve_existing_backup_path(
        &self,
        backup_file_name: &str,
    ) -> Result<PathBuf, DomainError> {
        let normalized = Self::normalize_backup_file_name(backup_file_name)?;
        let path = self.backups_dir.join(&normalized);
        if !path.starts_with(&self.backups_dir) {
            return Err(DomainError::InvalidData(
                "Invalid backup file name".to_string(),
            ));
        }

        Ok(path)
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

        if !sanitized.starts_with(Self::CHAT_BACKUP_PREFIX) {
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

    pub(super) fn get_group_backup_key(chat_id: &str) -> Result<String, DomainError> {
        Ok(format!(
            "group:{}",
            Self::normalize_jsonl_file_stem(chat_id)?
        ))
    }
}
