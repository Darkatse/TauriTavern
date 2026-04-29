use crate::domain::errors::DomainError;
use crate::domain::models::chat::{Chat, ChatMessage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub use super::chat_types::{
    ChatMessageReadItem, ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchHit,
    ChatMessageSearchQuery, ChatMessagesReadResult, ChatPayloadChunk, ChatPayloadCursor,
    ChatPayloadPatchOp, ChatPayloadTail, ChatSearchResult, FindLastMessageQuery,
    LocatedChatMessage, PinnedCharacterChat, PinnedGroupChat,
};

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
#[allow(clippy::upper_case_acronyms)]
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
    async fn save_with_options(&self, chat: &Chat, _force: bool) -> Result<(), DomainError> {
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

    /// List character chat summaries without loading full payloads.
    async fn list_chat_summaries(
        &self,
        character_filter: Option<&str>,
        include_metadata: bool,
    ) -> Result<Vec<ChatSearchResult>, DomainError>;

    /// List recent character chat summaries using non-full scan selection.
    async fn list_recent_chat_summaries(
        &self,
        character_filter: Option<&str>,
        include_metadata: bool,
        max_entries: usize,
        pinned: &[PinnedCharacterChat],
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

    /// List all chat backup files.
    async fn list_chat_backups(&self) -> Result<Vec<ChatSearchResult>, DomainError>;

    /// Get raw JSONL bytes for a chat backup file.
    async fn get_chat_backup_bytes(&self, backup_file_name: &str) -> Result<Vec<u8>, DomainError>;

    /// Delete a chat backup file.
    async fn delete_chat_backup(&self, backup_file_name: &str) -> Result<(), DomainError>;

    /// Get a raw chat JSONL payload for a character chat.
    async fn get_chat_payload(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Vec<Value>, DomainError>;

    /// Get raw JSONL bytes for a character chat payload.
    async fn get_chat_payload_bytes(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Vec<u8>, DomainError>;

    /// Get the absolute path to a character chat payload file.
    async fn get_chat_payload_path(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<PathBuf, DomainError>;

    /// Get the tail window for a character chat JSONL payload (excluding the header line).
    async fn get_chat_payload_tail_lines(
        &self,
        character_name: &str,
        file_name: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, DomainError>;

    /// Get JSONL lines before the current window cursor (excluding the header line).
    async fn get_chat_payload_before_lines(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError>;

    /// Save a windowed character chat payload by preserving bytes before cursor.offset and
    /// overwriting the tail from cursor.offset using the provided JSONL lines.
    async fn save_chat_payload_windowed(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        header: String,
        lines: Vec<String>,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError>;

    /// Patch a windowed character chat payload by applying an operation at the tail.
    async fn patch_chat_payload_windowed(
        &self,
        character_name: &str,
        file_name: &str,
        cursor: ChatPayloadCursor,
        header: String,
        op: ChatPayloadPatchOp,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError>;

    /// Save raw JSONL bytes for a character chat payload from an existing file path.
    async fn save_chat_payload_from_path(
        &self,
        character_name: &str,
        file_name: &str,
        source_path: &Path,
        force: bool,
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

    /// Get a single character chat summary without loading the full payload.
    async fn get_character_chat_summary(
        &self,
        character_name: &str,
        file_name: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError>;

    /// Read the character chat metadata (header only).
    async fn get_character_chat_metadata(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Value, DomainError>;

    /// Set `chat_metadata.extensions[namespace]` for a character chat (header-only rewrite).
    async fn set_character_chat_metadata_extension(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Read a JSON value from the character chat extension store.
    async fn get_character_chat_store_json(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Value, DomainError>;

    /// Write a JSON value to the character chat extension store.
    async fn set_character_chat_store_json(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Merge-update a JSON value in the character chat extension store.
    async fn update_character_chat_store_json(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Rename a JSON key in the character chat extension store.
    async fn rename_character_chat_store_key(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError>;

    /// Delete a JSON value from the character chat extension store.
    async fn delete_character_chat_store_json(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), DomainError>;

    /// List JSON keys in the character chat extension store for the namespace.
    async fn list_character_chat_store_keys(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
    ) -> Result<Vec<String>, DomainError>;

    /// Find the last message that matches the query in a character chat (tail scan).
    async fn find_last_character_chat_message(
        &self,
        character_name: &str,
        file_name: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, DomainError>;

    /// Read selected messages by absolute 0-based message index.
    async fn read_character_chat_messages(
        &self,
        character_name: &str,
        file_name: &str,
        indices: &[usize],
    ) -> Result<ChatMessagesReadResult, DomainError>;

    /// Search messages inside a character chat payload.
    async fn search_character_chat_messages(
        &self,
        character_name: &str,
        file_name: &str,
        query: ChatMessageSearchQuery,
    ) -> Result<Vec<ChatMessageSearchHit>, DomainError>;

    /// Clear the chat cache
    async fn clear_cache(&self) -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::ChatPayloadPatchOp;
    use serde_json::json;

    #[test]
    fn chat_payload_patch_op_deserializes_camel_case_start_index() {
        let op: ChatPayloadPatchOp = serde_json::from_value(json!({
            "kind": "rewriteFromIndex",
            "startIndex": 7,
            "lines": ["{\"hello\":\"world\"}"],
        }))
        .unwrap();

        match op {
            ChatPayloadPatchOp::RewriteFromIndex { start_index, lines } => {
                assert_eq!(start_index, 7);
                assert_eq!(lines, vec![String::from("{\"hello\":\"world\"}")]);
            }
            _ => panic!("Expected rewriteFromIndex op"),
        }
    }
}
