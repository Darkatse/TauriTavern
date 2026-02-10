use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Group model representing a character group in SillyTavern format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Unique identifier for the group
    pub id: String,

    /// Name of the group
    pub name: String,

    /// List of character avatars (filenames) that are members of this group
    #[serde(default)]
    pub members: Vec<String>,

    /// URL or path to the group's avatar image
    #[serde(default)]
    pub avatar_url: Option<String>,

    /// Whether characters can respond to themselves in the group chat
    #[serde(default)]
    pub allow_self_responses: bool,

    /// Strategy for activating characters in the group chat
    /// 0 = NATURAL, 1 = LIST, 2 = MANUAL, 3 = POOLED
    #[serde(default)]
    pub activation_strategy: i32,

    /// Mode for generating responses in the group chat
    /// 0 = SWAP, 1 = APPEND, 2 = APPEND_DISABLED
    #[serde(default)]
    pub generation_mode: i32,

    /// List of character avatars (filenames) that are disabled in this group
    #[serde(default)]
    pub disabled_members: Vec<String>,

    /// Metadata for the current chat
    #[serde(default)]
    pub chat_metadata: HashMap<String, serde_json::Value>,

    /// Whether the group is favorited
    #[serde(default)]
    pub fav: bool,

    /// ID of the current chat
    #[serde(default)]
    pub chat_id: String,

    /// List of all chat IDs associated with this group
    #[serde(default)]
    pub chats: Vec<String>,

    /// Delay in seconds for auto mode
    #[serde(default = "default_auto_mode_delay")]
    pub auto_mode_delay: i32,

    /// Prefix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_prefix: String,

    /// Suffix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_suffix: String,

    /// Whether to hide muted sprites
    #[serde(default = "default_true")]
    pub hide_muted_sprites: bool,

    /// Metadata for past chats
    #[serde(default)]
    pub past_metadata: HashMap<String, HashMap<String, serde_json::Value>>,

    /// Fields added by the backend for UI display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_added: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_date: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_size: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_last_chat: Option<i64>,
}

fn default_auto_mode_delay() -> i32 {
    5
}

fn default_true() -> bool {
    true
}

impl Group {
    /// Create a new group with basic information
    pub fn new(id: String, name: String, members: Vec<String>, avatar_url: Option<String>) -> Self {
        let chat_id = id.clone();

        Self {
            id,
            name,
            members,
            avatar_url,
            allow_self_responses: false,
            activation_strategy: 0, // NATURAL
            generation_mode: 0,     // SWAP
            disabled_members: Vec::new(),
            chat_metadata: HashMap::new(),
            fav: false,
            chat_id: chat_id.clone(),
            chats: vec![chat_id],
            auto_mode_delay: 5,
            generation_mode_join_prefix: String::new(),
            generation_mode_join_suffix: String::new(),
            hide_muted_sprites: true,
            past_metadata: HashMap::new(),
            date_added: None,
            create_date: None,
            chat_size: None,
            date_last_chat: None,
        }
    }
}
