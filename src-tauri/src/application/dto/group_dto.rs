use crate::domain::models::group::Group;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// DTO for group responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDto {
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
    #[serde(default)]
    pub activation_strategy: i32,

    /// Mode for generating responses in the group chat
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
    #[serde(default)]
    pub auto_mode_delay: i32,

    /// Prefix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_prefix: String,

    /// Suffix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_suffix: String,

    /// Whether to hide muted sprites
    #[serde(default)]
    pub hide_muted_sprites: bool,

    /// Metadata for past chats
    #[serde(default)]
    pub past_metadata: HashMap<String, HashMap<String, serde_json::Value>>,

    /// Creation timestamp in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_added: Option<i64>,

    /// Human-readable creation date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_date: Option<String>,

    /// Total size of all chat files in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_size: Option<u64>,

    /// Timestamp of the last chat in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_last_chat: Option<i64>,
}

/// DTO for creating a new group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupDto {
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
    #[serde(default)]
    pub activation_strategy: i32,

    /// Mode for generating responses in the group chat
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

    /// ID of the current chat (optional, will be generated if not provided)
    #[serde(default)]
    pub chat_id: Option<String>,

    /// List of all chat IDs associated with this group (optional)
    #[serde(default)]
    pub chats: Option<Vec<String>>,

    /// Delay in seconds for auto mode
    #[serde(default)]
    pub auto_mode_delay: Option<i32>,

    /// Prefix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_prefix: Option<String>,

    /// Suffix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_suffix: Option<String>,

    /// Whether to hide muted sprites
    #[serde(default)]
    pub hide_muted_sprites: Option<bool>,
}

/// DTO for updating a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateGroupDto {
    /// Unique identifier for the group
    pub id: String,

    /// Name of the group
    pub name: Option<String>,

    /// List of character avatars (filenames) that are members of this group
    #[serde(default)]
    pub members: Option<Vec<String>>,

    /// URL or path to the group's avatar image
    #[serde(default)]
    pub avatar_url: Option<String>,

    /// Whether characters can respond to themselves in the group chat
    #[serde(default)]
    pub allow_self_responses: Option<bool>,

    /// Strategy for activating characters in the group chat
    #[serde(default)]
    pub activation_strategy: Option<i32>,

    /// Mode for generating responses in the group chat
    #[serde(default)]
    pub generation_mode: Option<i32>,

    /// List of character avatars (filenames) that are disabled in this group
    #[serde(default)]
    pub disabled_members: Option<Vec<String>>,

    /// Metadata for the current chat
    #[serde(default)]
    pub chat_metadata: Option<HashMap<String, serde_json::Value>>,

    /// Whether the group is favorited
    #[serde(default)]
    pub fav: Option<bool>,

    /// ID of the current chat
    #[serde(default)]
    pub chat_id: Option<String>,

    /// List of all chat IDs associated with this group
    #[serde(default)]
    pub chats: Option<Vec<String>>,

    /// Delay in seconds for auto mode
    #[serde(default)]
    pub auto_mode_delay: Option<i32>,

    /// Prefix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_prefix: Option<String>,

    /// Suffix for joining messages in APPEND mode
    #[serde(default)]
    pub generation_mode_join_suffix: Option<String>,

    /// Whether to hide muted sprites
    #[serde(default)]
    pub hide_muted_sprites: Option<bool>,

    /// Metadata for past chats
    #[serde(default)]
    pub past_metadata: Option<HashMap<String, HashMap<String, serde_json::Value>>>,
}

/// DTO for deleting a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteGroupDto {
    /// Unique identifier for the group to delete
    pub id: String,
}

// Conversion implementations
impl From<Group> for GroupDto {
    fn from(group: Group) -> Self {
        Self {
            id: group.id,
            name: group.name,
            members: group.members,
            avatar_url: group.avatar_url,
            allow_self_responses: group.allow_self_responses,
            activation_strategy: group.activation_strategy,
            generation_mode: group.generation_mode,
            disabled_members: group.disabled_members,
            chat_metadata: group.chat_metadata,
            fav: group.fav,
            chat_id: group.chat_id,
            chats: group.chats,
            auto_mode_delay: group.auto_mode_delay,
            generation_mode_join_prefix: group.generation_mode_join_prefix,
            generation_mode_join_suffix: group.generation_mode_join_suffix,
            hide_muted_sprites: group.hide_muted_sprites,
            past_metadata: group.past_metadata,
            date_added: group.date_added,
            create_date: group.create_date,
            chat_size: group.chat_size,
            date_last_chat: group.date_last_chat,
        }
    }
}
