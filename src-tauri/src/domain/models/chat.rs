use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc, Local, TimeZone};
use uuid::Uuid;
use std::collections::HashMap;

/// Format a date in the SillyTavern format (YYYY-MM-DD@HHhMMmSSs)
pub fn humanized_date(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    local.format("%Y-%m-%d@%Hh%Mm%Ss").to_string()
}

/// Format a date in ISO8601 format with local timezone
pub fn humanized_iso8601_date_time() -> String {
    let now = Local::now();
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Format a date in the SillyTavern message format (Month DD, YYYY HH:MMam/pm)
pub fn message_date_format(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    local.format("%B %d, %Y %l:%M%P").to_string().trim().to_string()
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

    #[serde(default)]
    pub timedWorldInfo: TimedWorldInfo,

    #[serde(default)]
    pub variables: HashMap<String, String>,

    #[serde(default)]
    pub tainted: bool,

    #[serde(default)]
    pub lastInContextMessageId: u32,

    #[serde(default)]
    pub chat_instruct: Option<bool>,

    #[serde(default)]
    pub chat_completions: Option<bool>,

    #[serde(default)]
    pub extensions: Option<HashMap<String, serde_json::Value>>,
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

    #[serde(default)]
    pub isSmallSys: Option<bool>,

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
        }
    }

    /// Create a new system message
    pub fn system(content: &str) -> Self {
        Self {
            name: "System".to_string(),
            is_user: false,
            is_system: true,
            send_date: message_date_format(Utc::now()),
            mes: content.to_string(),
            extra: MessageExtra::default(),
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

    /// Get the number of messages in the chat
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get the file name for this chat
    pub fn get_file_name(&self) -> String {
        if let Some(file_name) = &self.file_name {
            format!("{}.jsonl", file_name)
        } else {
            format!("{} - {}.jsonl", self.character_name, self.create_date)
        }
    }

    /// Get a preview of the last message
    pub fn get_preview(&self) -> String {
        if let Some(last) = self.last_message() {
            let preview = last.mes.replace("\r", " ").replace("\n", " ");
            if preview.len() > 100 {
                format!("{}...", &preview[0..97])
            } else {
                preview
            }
        } else {
            "No messages".to_string()
        }
    }

    /// Get the last message date as a timestamp
    pub fn get_last_message_timestamp(&self) -> i64 {
        if let Some(last) = self.last_message() {
            // Try to parse the send_date
            if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&last.send_date, "%B %d, %Y %l:%M%P") {
                if let Some(dt) = Local.from_local_datetime(&dt).single() {
                    return dt.timestamp_millis();
                }
            }
        }
        0
    }
}
