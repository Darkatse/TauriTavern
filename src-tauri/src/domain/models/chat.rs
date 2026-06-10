use chrono::{DateTime, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::domain::models::filename::{
    MAX_SANITIZED_FILENAME_BYTES, sanitize_filename, truncate_utf8_bytes,
};

const CHAT_FILE_EXTENSION: &str = ".jsonl";
const MAX_CHAT_FILE_STEM_BYTES: usize = MAX_SANITIZED_FILENAME_BYTES - CHAT_FILE_EXTENSION.len();

/// Format a date in the SillyTavern format (YYYY-MM-DD@HHhMMmSSs)
pub fn humanized_date(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    local.format("%Y-%m-%d@%Hh%Mm%Ss").to_string()
}

/// Remove the final lowercase `.jsonl` suffix when present.
///
/// This intentionally mirrors SillyTavern's case-sensitive chat file contract:
/// `Story.JSONL` is a stem, not an extension.
pub fn strip_jsonl_extension(file_name: &str) -> &str {
    file_name
        .strip_suffix(CHAT_FILE_EXTENSION)
        .unwrap_or(file_name)
}

fn sanitize_chat_file_name(file_name: &str) -> String {
    let stem = strip_jsonl_extension(file_name);
    sanitize_filename(&format!("{stem}{CHAT_FILE_EXTENSION}"))
}

/// Normalize a chat payload file name using SillyTavern's full `name + .jsonl` contract.
///
/// Returns `None` when sanitization would erase the stem or truncate away the
/// required `.jsonl` suffix. Callers should surface that as a validation error
/// instead of falling back to a synthetic chat name.
pub fn normalize_chat_file_name(file_name: &str) -> Option<String> {
    let normalized = sanitize_chat_file_name(file_name);
    if !normalized.ends_with(CHAT_FILE_EXTENSION) {
        return None;
    }

    let stem = strip_jsonl_extension(&normalized);
    if stem.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Normalize a chat payload file stem without applying repository fallbacks.
pub fn normalize_chat_file_stem(file_name: &str) -> Option<String> {
    normalize_chat_file_name(file_name)
        .map(|normalized| strip_jsonl_extension(&normalized).to_string())
}

/// Truncate an already-sanitized stem prefix while reserving byte space for a suffix.
pub(crate) fn truncate_chat_file_stem_prefix<'a>(prefix: &'a str, suffix: &str) -> &'a str {
    let max_prefix_bytes = MAX_CHAT_FILE_STEM_BYTES.saturating_sub(suffix.len());
    truncate_utf8_bytes(prefix, max_prefix_bytes)
}

/// Format a date in the SillyTavern message format (Month DD, YYYY HH:MMam/pm)
pub fn message_date_format(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    local
        .format("%B %d, %Y %l:%M%P")
        .to_string()
        .trim()
        .to_string()
}

/// Chat metadata structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMetadata {
    #[serde(default)]
    pub chat_id_hash: u64,

    #[serde(default)]
    pub note_prompt: String,

    #[serde(default)]
    pub note_interval: u32,

    #[serde(default)]
    pub note_position: u32,

    #[serde(default)]
    pub note_depth: u32,

    #[serde(default)]
    pub note_role: u32,

    #[serde(default, rename = "timedWorldInfo")]
    pub timed_world_info: TimedWorldInfo,

    #[serde(default)]
    pub variables: HashMap<String, String>,

    #[serde(default)]
    pub tainted: bool,

    #[serde(default, rename = "lastInContextMessageId")]
    pub last_in_context_message_id: u32,

    #[serde(default)]
    pub chat_instruct: Option<bool>,

    #[serde(default)]
    pub chat_completions: Option<bool>,

    #[serde(default)]
    pub extensions: Option<HashMap<String, serde_json::Value>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,

    #[serde(default, flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// Timed world info structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimedWorldInfo {
    #[serde(default)]
    pub sticky: HashMap<String, serde_json::Value>,

    #[serde(default)]
    pub cooldown: HashMap<String, serde_json::Value>,
}

/// Chat message extra data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageExtra {
    #[serde(default)]
    pub api: Option<String>,

    #[serde(default)]
    pub model: Option<String>,

    #[serde(default)]
    pub reasoning: Option<String>,

    #[serde(default)]
    pub reasoning_duration: Option<u64>,

    #[serde(default)]
    pub token_count: Option<u32>,

    #[serde(default, rename = "isSmallSys")]
    pub is_small_sys: Option<bool>,

    #[serde(default)]
    pub gen_started: Option<String>,

    #[serde(default)]
    pub gen_finished: Option<String>,

    #[serde(default)]
    pub swipe_id: Option<u32>,

    #[serde(default)]
    pub swipes: Option<Vec<String>>,

    #[serde(default)]
    pub swipe_info: Option<Vec<serde_json::Value>>,

    #[serde(default)]
    pub title: Option<String>,

    #[serde(default)]
    pub force_avatar: Option<String>,

    #[serde(default, flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// Chat message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub is_user: bool,

    #[serde(default)]
    pub is_system: bool,

    #[serde(default)]
    pub send_date: String,

    #[serde(default)]
    pub mes: String,

    #[serde(default)]
    pub extra: MessageExtra,

    #[serde(default, flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

impl ChatMessage {
    /// Create a new user message
    pub fn user(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            is_user: true,
            is_system: false,
            send_date: message_date_format(Utc::now()),
            mes: content.to_string(),
            extra: MessageExtra::default(),
            additional: HashMap::new(),
        }
    }

    /// Create a new character message
    pub fn character(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            is_user: false,
            is_system: false,
            send_date: message_date_format(Utc::now()),
            mes: content.to_string(),
            extra: MessageExtra::default(),
            additional: HashMap::new(),
        }
    }
}

/// Chat structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Chat {
    #[serde(default)]
    pub user_name: String,

    #[serde(default)]
    pub character_name: String,

    #[serde(default)]
    pub create_date: String,

    #[serde(default)]
    pub chat_metadata: ChatMetadata,

    #[serde(default)]
    pub messages: Vec<ChatMessage>,

    #[serde(skip)]
    pub file_name: Option<String>,
}

impl Chat {
    /// Create a new chat
    pub fn new(user_name: &str, character_name: &str) -> Self {
        let now = Utc::now();
        let create_date = humanized_date(now); // `create_date` 拥有 String
        let chat_id_hash = rand::random::<u64>();

        // 在移动 create_date 之前，先用它计算 file_name
        let file_name = Some(format!("{} - {}", character_name, create_date));

        Self {
            user_name: user_name.to_string(),
            character_name: character_name.to_string(),
            create_date,
            chat_metadata: ChatMetadata {
                chat_id_hash,
                ..Default::default()
            },
            messages: Vec::new(),
            file_name,
        }
    }

    /// Add a message to the chat
    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
    }

    /// Get the last message in the chat
    pub fn last_message(&self) -> Option<&ChatMessage> {
        self.messages.last()
    }

    /// Get the last message date as a timestamp
    pub fn get_last_message_timestamp(&self) -> i64 {
        if let Some(last) = self.last_message() {
            return parse_message_timestamp(&last.send_date);
        }
        0
    }
}

fn normalize_epoch_millis(value: i64) -> i64 {
    if value.abs() < 1_000_000_000_000 {
        value.saturating_mul(1000)
    } else {
        value
    }
}

pub fn parse_message_timestamp(send_date: &str) -> i64 {
    let raw = send_date.trim();
    if raw.is_empty() {
        return 0;
    }

    if let Ok(epoch) = raw.parse::<i64>() {
        return normalize_epoch_millis(epoch);
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return dt.timestamp_millis();
    }

    let local_formats = [
        "%B %d, %Y %l:%M%P",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d@%Hh%Mm%Ss%fms",
        "%Y-%m-%d@%Hh%Mm%Ss",
        "%Y-%m-%d @%Hh %Mm %Ss %fms",
        "%Y-%m-%d @%Hh %Mm %Ss",
    ];

    for fmt in local_formats {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(raw, fmt) {
            if let Some(local_dt) = Local.from_local_datetime(&dt).single() {
                return local_dt.timestamp_millis();
            }
        }
    }

    0
}

pub fn parse_message_timestamp_value(send_date: Option<&Value>) -> i64 {
    match send_date {
        Some(Value::Number(number)) => {
            if let Some(v) = number.as_i64() {
                normalize_epoch_millis(v)
            } else if let Some(v) = number.as_u64() {
                normalize_epoch_millis(v as i64)
            } else {
                0
            }
        }
        Some(Value::String(text)) => parse_message_timestamp(text),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_chat_file_name, normalize_chat_file_stem, parse_message_timestamp,
        parse_message_timestamp_value, strip_jsonl_extension, truncate_chat_file_stem_prefix,
    };
    use serde_json::json;

    #[test]
    fn parses_rfc3339_timestamp() {
        let timestamp = parse_message_timestamp("2026-02-11T02:26:58.931Z");
        assert!(timestamp > 0);
    }

    #[test]
    fn parses_legacy_humanized_timestamp() {
        let timestamp = parse_message_timestamp("October 29, 2025 9:35pm");
        assert!(timestamp > 0);
    }

    #[test]
    fn normalizes_epoch_seconds_from_json_number() {
        let send_date = json!(1_700_000_000);
        let timestamp = parse_message_timestamp_value(Some(&send_date));
        assert_eq!(timestamp, 1_700_000_000_000);
    }

    #[test]
    fn strips_jsonl_extension_like_upstream_case_sensitively() {
        assert_eq!(strip_jsonl_extension("Chat.jsonl"), "Chat");
        assert_eq!(strip_jsonl_extension("Chat.JSONL"), "Chat.JSONL");
        assert_eq!(strip_jsonl_extension("Chat.json"), "Chat.json");
    }

    #[test]
    fn sanitizes_chat_file_stem_without_repository_fallback() {
        assert_eq!(
            normalize_chat_file_name("Story:One.jsonl"),
            Some("StoryOne.jsonl".to_string())
        );
        assert_eq!(
            normalize_chat_file_stem("Story:One.jsonl"),
            Some("StoryOne".to_string())
        );
        assert_eq!(
            normalize_chat_file_stem("/ Story.jsonl"),
            Some(" Story".to_string())
        );
        assert_eq!(
            normalize_chat_file_stem("中文会话.jsonl"),
            Some("中文会话".to_string())
        );
        assert_eq!(normalize_chat_file_stem("CON.jsonl"), None);
        assert_eq!(normalize_chat_file_stem("*.jsonl"), None);
    }

    #[test]
    fn keeps_uppercase_jsonl_as_part_of_the_chat_stem() {
        assert_eq!(
            normalize_chat_file_name("Story.JSONL"),
            Some("Story.JSONL.jsonl".to_string())
        );
        assert_eq!(
            normalize_chat_file_stem("Story.JSONL"),
            Some("Story.JSONL".to_string())
        );
    }

    #[test]
    fn preserves_upstream_chat_file_spacing_semantics() {
        assert_eq!(
            normalize_chat_file_name(" Story.jsonl"),
            Some(" Story.jsonl".to_string())
        );
        assert_eq!(
            normalize_chat_file_stem(" Story.jsonl"),
            Some(" Story".to_string())
        );
        assert_eq!(
            normalize_chat_file_name("Story .jsonl"),
            Some("Story .jsonl".to_string())
        );
        assert_eq!(
            normalize_chat_file_stem("Story .jsonl"),
            Some("Story ".to_string())
        );
        assert_eq!(
            normalize_chat_file_name("session.jsonl "),
            Some("session.jsonl .jsonl".to_string())
        );
        assert_eq!(
            normalize_chat_file_stem("session.jsonl "),
            Some("session.jsonl ".to_string())
        );
    }

    #[test]
    fn normalizes_complete_chat_file_name_before_utf8_byte_truncation() {
        let max_stem = "a".repeat(249);
        assert_eq!(
            normalize_chat_file_name(&max_stem),
            Some(format!("{}.jsonl", max_stem))
        );
        assert_eq!(normalize_chat_file_stem(&max_stem), Some(max_stem));

        let overflowing_stem = "a".repeat(250);
        assert_eq!(normalize_chat_file_name(&overflowing_stem), None);
        assert_eq!(normalize_chat_file_stem(&overflowing_stem), None);
    }

    #[test]
    fn truncates_generated_chat_stem_prefix_without_splitting_utf8() {
        let prefix = "中".repeat(100);
        let suffix = " - imported";
        let truncated = truncate_chat_file_stem_prefix(&prefix, suffix);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(format!("{}{}.jsonl", truncated, suffix).len() <= 255);
    }
}
