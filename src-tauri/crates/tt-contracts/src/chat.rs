use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatImportFormat {
    SillyTavern,
    Ooba,
    Agnai,
    CAITools,
    KoboldLite,
    RisuAI,
}

impl From<String> for ChatImportFormat {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "sillytavern" => ChatImportFormat::SillyTavern,
            "ooba" => ChatImportFormat::Ooba,
            "agnai" => ChatImportFormat::Agnai,
            "caitools" => ChatImportFormat::CAITools,
            "koboldlite" => ChatImportFormat::KoboldLite,
            "risuai" => ChatImportFormat::RisuAI,
            _ => ChatImportFormat::SillyTavern,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum ChatExportFormat {
    JSONL,
    PlainText,
}

impl From<String> for ChatExportFormat {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "plaintext" => ChatExportFormat::PlainText,
            _ => ChatExportFormat::JSONL,
        }
    }
}

/// Chat search result.
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

/// Metadata-only entry for the chat backup catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatBackupCatalogEntry {
    pub file_name: String,
    pub stored_size: u64,
    pub backup_date: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_count: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatBackupStorageStats {
    pub original_bytes: u64,
    pub stored_bytes: u64,
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

/// Cursor for paged JSONL chat payload reads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayloadCursor {
    pub offset: u64,
    pub size: u64,
    pub modified_millis: i64,
}

/// Tail page for a chat JSONL payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayloadTail {
    pub header: String,
    pub lines: Vec<String>,
    pub cursor: ChatPayloadCursor,
    pub has_more_before: bool,
}

/// Chunk returned for pagination requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPayloadChunk {
    pub lines: Vec<String>,
    pub cursor: ChatPayloadCursor,
    pub has_more_before: bool,
}

/// Chat message role used by locate, read, and search operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ChatMessageRole {
    User,
    Assistant,
    System,
    Tool,
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
    /// Internal text view; never accepted from serialized requests.
    #[serde(skip)]
    pub frozen_macros: Option<std::sync::Arc<tt_domain::frozen_macros::FrozenMacros>>,
    pub query: String,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<ChatMessageSearchFilters>,
}

/// One chat message loaded by absolute 0-based message index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageReadItem {
    pub index: usize,
    pub role: ChatMessageRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_date: Option<String>,
    pub text: String,
}

/// Result for reading selected messages from a chat payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessagesReadResult {
    pub total_messages: usize,
    pub messages: Vec<ChatMessageReadItem>,
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
