use serde::{Deserialize, Serialize};
use serde_json::Value;

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
