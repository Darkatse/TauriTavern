use crate::domain::errors::DomainError;
use crate::domain::models::chat::{Chat, ChatMessage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Chat search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSearchResult {
    pub character_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub message_count: usize,
    pub preview: String,
    pub date: i64,
    pub chat_id: Option<String>,
}

/// Chat import format
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatImportFormat {
    SillyTavern,
    Ooba,
    Agnai,
    CAITools,
    KoboldLite,
    RisuAI,
}

/// Chat export format
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatExportFormat {
    JSONL,
    PlainText,
}

/// Repository interface for chat management
#[async_trait]
pub trait ChatRepository: Send + Sync {
    /// Save a chat to the repository
    async fn save(&self, chat: &Chat) -> Result<(), DomainError>;

    /// Save a chat with explicit overwrite/integrity options.
    async fn save_with_options(&self, chat: &Chat, force: bool) -> Result<(), DomainError> {
        let _ = force;
        self.save(chat).await
    }

    /// Get a chat by character name and file name
    async fn get_chat(&self, character_name: &str, file_name: &str) -> Result<Chat, DomainError>;

    /// Get all chats for a character
    async fn get_character_chats(&self, character_name: &str) -> Result<Vec<Chat>, DomainError>;

    /// Get all chats
    async fn get_all_chats(&self) -> Result<Vec<Chat>, DomainError>;

    /// Delete a chat
    async fn delete_chat(&self, character_name: &str, file_name: &str) -> Result<(), DomainError>;

    /// Rename a chat
    async fn rename_chat(
        &self,
        character_name: &str,
        old_file_name: &str,
        new_file_name: &str,
    ) -> Result<(), DomainError>;

    /// Add a message to a chat
    async fn add_message(
        &self,
        character_name: &str,
        file_name: &str,
        message: ChatMessage,
    ) -> Result<Chat, DomainError>;

    /// Search for chats
    async fn search_chats(
        &self,
        query: &str,
        character_filter: Option<&str>,
    ) -> Result<Vec<ChatSearchResult>, DomainError>;

    /// Import a chat from a file
    async fn import_chat(
        &self,
        character_name: &str,
        file_path: &Path,
        format: ChatImportFormat,
    ) -> Result<Chat, DomainError>;

    /// Export a chat to a file
    async fn export_chat(
        &self,
        character_name: &str,
        file_name: &str,
        target_path: &Path,
        format: ChatExportFormat,
    ) -> Result<(), DomainError>;

    /// Backup a chat
    async fn backup_chat(&self, character_name: &str, file_name: &str) -> Result<(), DomainError>;

    /// Get a raw chat JSONL payload for a character chat.
    async fn get_chat_payload(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Vec<Value>, DomainError>;

    /// Save a raw chat JSONL payload for a character chat.
    async fn save_chat_payload(
        &self,
        character_name: &str,
        file_name: &str,
        payload: &[Value],
        force: bool,
    ) -> Result<(), DomainError>;

    /// Get a raw chat JSONL payload for a group chat.
    async fn get_group_chat_payload(&self, chat_id: &str) -> Result<Vec<Value>, DomainError>;

    /// Save a raw chat JSONL payload for a group chat.
    async fn save_group_chat_payload(
        &self,
        chat_id: &str,
        payload: &[Value],
        force: bool,
    ) -> Result<(), DomainError>;

    /// Delete a group chat payload file.
    async fn delete_group_chat_payload(&self, chat_id: &str) -> Result<(), DomainError>;

    /// Rename a group chat payload file.
    async fn rename_group_chat_payload(
        &self,
        old_file_name: &str,
        new_file_name: &str,
    ) -> Result<(), DomainError>;

    /// Import character chat file(s) and return created JSONL file names.
    async fn import_chat_payload(
        &self,
        character_name: &str,
        character_display_name: &str,
        user_name: &str,
        file_path: &Path,
        format: &str,
    ) -> Result<Vec<String>, DomainError>;

    /// Import a group chat payload and return the created chat id (without extension).
    async fn import_group_chat_payload(&self, file_path: &Path) -> Result<String, DomainError>;

    /// Clear the chat cache
    async fn clear_cache(&self) -> Result<(), DomainError>;
}
