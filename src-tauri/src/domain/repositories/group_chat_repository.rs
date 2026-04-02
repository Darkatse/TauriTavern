use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::domain::errors::DomainError;

use super::chat_types::{
    ChatMessageSearchHit, ChatMessageSearchQuery, ChatPayloadChunk, ChatPayloadCursor,
    ChatPayloadPatchOp, ChatPayloadTail, ChatSearchResult, FindLastMessageQuery,
    LocatedChatMessage, PinnedGroupChat,
};

/// Repository interface for group chat (JSONL payload) management.
#[async_trait]
pub trait GroupChatRepository: Send + Sync {
    /// List group chat summaries without loading full payloads.
    async fn list_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
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

    /// Import a group chat payload and return the created chat id (without extension).
    async fn import_group_chat_payload(&self, file_path: &Path) -> Result<String, DomainError>;

    /// Get a single group chat summary without loading the full payload.
    async fn get_group_chat_summary(
        &self,
        chat_id: &str,
        include_metadata: bool,
    ) -> Result<ChatSearchResult, DomainError>;

    /// Read the group chat metadata (header only).
    async fn get_group_chat_metadata(&self, chat_id: &str) -> Result<Value, DomainError>;

    /// Set `chat_metadata.extensions[namespace]` for a group chat (header-only rewrite).
    async fn set_group_chat_metadata_extension(
        &self,
        chat_id: &str,
        namespace: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Read a JSON value from the group chat extension store.
    async fn get_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Value, DomainError>;

    /// Write a JSON value to the group chat extension store.
    async fn set_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Merge-update a JSON value in the group chat extension store.
    async fn update_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    /// Rename a JSON key in the group chat extension store.
    async fn rename_group_chat_store_key(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError>;

    /// Delete a JSON value from the group chat extension store.
    async fn delete_group_chat_store_json(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), DomainError>;

    /// List JSON keys in the group chat extension store for the namespace.
    async fn list_group_chat_store_keys(
        &self,
        chat_id: &str,
        namespace: &str,
    ) -> Result<Vec<String>, DomainError>;

    /// Find the last message that matches the query in a group chat (tail scan).
    async fn find_last_group_chat_message(
        &self,
        chat_id: &str,
        query: FindLastMessageQuery,
    ) -> Result<Option<LocatedChatMessage>, DomainError>;

    /// Search messages inside a group chat payload.
    async fn search_group_chat_messages(
        &self,
        chat_id: &str,
        query: ChatMessageSearchQuery,
    ) -> Result<Vec<ChatMessageSearchHit>, DomainError>;

    /// Clear any cached data for group chats.
    async fn clear_cache(&self) -> Result<(), DomainError>;
}
