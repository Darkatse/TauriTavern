use crate::domain::errors::DomainError;
use crate::domain::models::chat::{Chat, ChatMessage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Chat search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSearchResult {
    pub character_name: String,
    pub file_name: String,
    pub file_size: u64,
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

    /// Clear the chat cache
    async fn clear_cache(&self) -> Result<(), DomainError>;
}
