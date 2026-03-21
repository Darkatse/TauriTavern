use crate::domain::errors::DomainError;
use crate::domain::models::chat::{Chat, ChatMessage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<Value>,
}

/// Pinned character chat reference used by recent-chat queries.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct PinnedCharacterChat {
    pub character_name: String,
    pub file_name: String,
}

/// Pinned group chat reference used by recent-chat queries.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct PinnedGroupChat {
    pub chat_id: String,
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
#[allow(clippy::upper_case_acronyms)]
pub enum ChatExportFormat {
    JSONL,
    PlainText,
}

/// Cursor for windowed JSONL chat payload operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayloadCursor {
    pub offset: u64,
    pub size: u64,
    pub modified_millis: i64,
}

/// Tail window for a chat JSONL payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayloadTail {
    pub header: String,
    pub lines: Vec<String>,
    pub cursor: ChatPayloadCursor,
    pub has_more_before: bool,
}

/// Window chunk returned for pagination requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayloadChunk {
    pub lines: Vec<String>,
    pub cursor: ChatPayloadCursor,
    pub has_more_before: bool,
}

/// Operation-based patch for windowed JSONL payload writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ChatPayloadPatchOp {
    /// Append message lines at the end of the payload (excluding the header line).
    Append { lines: Vec<String> },
    /// Rewrite the payload tail starting at `start_index` (0-based, relative to cursor.offset),
    /// replacing everything from that line through EOF with `lines`.
    RewriteFromIndex {
        #[serde(rename = "startIndex")]
        start_index: usize,
        lines: Vec<String>,
    },
}

/// Chat message role used for locate queries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ChatMessageRole {
    User,
    Assistant,
    System,
}

/// Query for locating the last matching message in a chat payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindLastMessageQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ChatMessageRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_top_level_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_extra_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_limit: Option<usize>,
}

/// Located message result with a 0-based absolute message index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocatedChatMessage {
    pub index: usize,
    pub message: Value,
}

/// Filters for chat message search queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageSearchFilters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ChatMessageRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
    /// Maximum number of messages scanned from the end of the chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_limit: Option<usize>,
}

/// Query payload for searching messages inside a chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageSearchQuery {
    pub query: String,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<ChatMessageSearchFilters>,
}

/// Search hit returned for chat message search queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageSearchHit {
    pub index: usize,
    pub score: f32,
    pub snippet: String,
    pub role: ChatMessageRole,
    pub text: String,
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

    /// List group chat summaries without loading full payloads.
    async fn list_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
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

    /// List recent group chat summaries using non-full scan selection.
    async fn list_recent_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
        max_entries: usize,
        pinned: &[PinnedGroupChat],
    ) -> Result<Vec<ChatSearchResult>, DomainError>;

    /// Search group chats with optional chat id filter.
    async fn search_group_chats(
        &self,
        query: &str,
        chat_ids: Option<&[String]>,
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

    /// Get the absolute path to a group chat payload file.
    async fn get_group_chat_payload_path(&self, chat_id: &str) -> Result<PathBuf, DomainError>;

    /// Get the tail window for a group chat JSONL payload (excluding the header line).
    async fn get_group_chat_payload_tail_lines(
        &self,
        chat_id: &str,
        max_lines: usize,
    ) -> Result<ChatPayloadTail, DomainError>;

    /// Get JSONL lines before the current group chat window cursor (excluding the header line).
    async fn get_group_chat_payload_before_lines(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        max_lines: usize,
    ) -> Result<ChatPayloadChunk, DomainError>;

    /// Save a windowed group chat payload by preserving bytes before cursor.offset and
    /// overwriting the tail from cursor.offset using the provided JSONL lines.
    async fn save_group_chat_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        lines: Vec<String>,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError>;

    /// Patch a windowed group chat payload by applying an operation at the tail.
    async fn patch_group_chat_payload_windowed(
        &self,
        chat_id: &str,
        cursor: ChatPayloadCursor,
        header: String,
        op: ChatPayloadPatchOp,
        force: bool,
    ) -> Result<ChatPayloadCursor, DomainError>;

    /// Save raw JSONL bytes for a group chat payload from an existing file path.
    async fn save_group_chat_payload_from_path(
        &self,
        chat_id: &str,
        source_path: &Path,
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

    /// Get a single character chat summary without loading the full payload.
    async fn get_character_chat_summary(
        &self,
        character_name: &str,
        file_name: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError>;

    /// Get a single group chat summary without loading the full payload.
    async fn get_group_chat_summary(
        &self,
        chat_id: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError>;

    /// Read the character chat metadata (header only).
    async fn get_character_chat_metadata(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Value, DomainError>;

    /// Read the group chat metadata (header only).
    async fn get_group_chat_metadata(&self, chat_id: &str) -> Result<Value, DomainError>;

    /// Set `chat_metadata.extensions[namespace]` for a character chat (header-only rewrite).
    async fn set_character_chat_metadata_extension(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Set `chat_metadata.extensions[namespace]` for a group chat (header-only rewrite).
    async fn set_group_chat_metadata_extension(
        &self,
        chat_id: &str,
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

    /// Read a JSON value from the group chat extension store.
    async fn get_group_chat_store_json(
        &self,
        chat_id: &str,
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

    /// Write a JSON value to the group chat extension store.
    async fn set_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Delete a JSON value from the character chat extension store.
    async fn delete_character_chat_store_json(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), DomainError>;

    /// Delete a JSON value from the group chat extension store.
    async fn delete_group_chat_store_json(
        &self,
        chat_id: &str,
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

    /// List JSON keys in the group chat extension store for the namespace.
    async fn list_group_chat_store_keys(
        &self,
        chat_id: &str,
        namespace: &str,
    ) -> Result<Vec<String>, DomainError>;

    /// Find the last message that matches the query in a character chat (tail scan).
    async fn find_last_character_chat_message(
        &self,
        character_name: &str,
        file_name: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, DomainError>;

    /// Find the last message that matches the query in a group chat (tail scan).
    async fn find_last_group_chat_message(
        &self,
        chat_id: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, DomainError>;

    /// Search messages inside a character chat payload.
    async fn search_character_chat_messages(
        &self,
        character_name: &str,
        file_name: &str,
        query: ChatMessageSearchQuery,
    ) -> Result<Vec<ChatMessageSearchHit>, DomainError>;

    /// Search messages inside a group chat payload.
    async fn search_group_chat_messages(
        &self,
        chat_id: &str,
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
