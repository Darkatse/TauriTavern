use std::path::PathBuf;

use chrono::Local;
use tokio::fs;

use crate::domain::errors::DomainError;

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

    /// Get the path to a character's chat directory
    pub(super) fn get_character_dir(&self, character_name: &str) -> PathBuf {
        self.chats_dir.join(character_name)
    }

    /// Ensure chat file names always use the JSONL extension
    pub(super) fn normalize_jsonl_file_name(file_name: &str) -> String {
        if file_name.ends_with(".jsonl") {
            file_name.to_string()
        } else {
            format!("{}.jsonl", file_name)
        }
    }

    /// Remove JSONL extension if present
    pub(super) fn strip_jsonl_extension(file_name: &str) -> &str {
        file_name.strip_suffix(".jsonl").unwrap_or(file_name)
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

    /// Get the path to a chat file
    pub(super) fn get_chat_path(&self, character_name: &str, file_name: &str) -> PathBuf {
        let normalized = Self::normalize_jsonl_file_name(file_name);
        self.get_character_dir(character_name).join(normalized)
    }

    /// Get the path to a group chat file
    pub(super) fn get_group_chat_path(&self, chat_id: &str) -> PathBuf {
        let normalized = Self::normalize_jsonl_file_name(chat_id);
        self.group_chats_dir.join(normalized)
    }

    /// Get the path to a chat backup file
    pub(super) fn get_backup_path(&self, backup_name: &str) -> PathBuf {
        self.backups_dir.join(Self::backup_file_name(backup_name))
    }

    /// Get the cache key for a chat
    pub(super) fn get_cache_key(&self, character_name: &str, file_name: &str) -> String {
        format!(
            "{}:{}",
            character_name,
            Self::strip_jsonl_extension(file_name)
        )
    }
}
