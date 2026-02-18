use crate::domain::models::chat::{Chat, ChatMessage, MessageExtra};
use crate::domain::repositories::chat_repository::{
    ChatExportFormat, ChatImportFormat, ChatSearchResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// DTO for chat message extra data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageExtraDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_duration: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isSmallSys: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_started: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_finished: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swipe_id: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swipes: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swipe_info: Option<Vec<serde_json::Value>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_avatar: Option<String>,

    #[serde(default, flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// DTO for chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageDto {
    pub name: String,
    pub is_user: bool,
    pub is_system: bool,
    pub send_date: String,
    pub mes: String,
    pub extra: MessageExtraDto,

    #[serde(default, flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// DTO for chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDto {
    pub character_name: String,
    pub user_name: String,
    pub file_name: String,
    pub create_date: String,
    pub chat_metadata: serde_json::Value,
    pub messages: Vec<ChatMessageDto>,
    pub message_count: usize,
    pub chat_id: u64,
}

/// DTO for chat search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSearchResultDto {
    pub character_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub message_count: usize,
    pub preview: String,
    pub date: i64,
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_metadata: Option<serde_json::Value>,
}

/// DTO for creating a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChatDto {
    pub character_name: String,
    pub user_name: String,
    pub first_message: Option<String>,
}

/// DTO for adding a message to a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddMessageDto {
    pub character_name: String,
    pub file_name: String,
    pub is_user: bool,
    pub content: String,
    pub extra: Option<MessageExtraDto>,
}

/// DTO for renaming a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameChatDto {
    pub character_name: String,
    pub old_file_name: String,
    pub new_file_name: String,
}

/// DTO for importing a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportChatDto {
    pub character_name: String,
    pub file_path: String,
    pub format: String,
}

/// DTO for exporting a chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportChatDto {
    pub character_name: String,
    pub file_name: String,
    pub target_path: String,
    pub format: String,
}

/// DTO for saving a full chat payload (metadata + messages)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveChatDto {
    #[serde(rename = "ch_name")]
    pub character_name: String,
    pub file_name: String,
    pub chat: Vec<serde_json::Value>,
    pub force: Option<bool>,
}

/// DTO for saving a character chat payload from an existing JSONL file path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveChatFromFileDto {
    #[serde(rename = "ch_name")]
    pub character_name: String,
    pub file_name: String,
    pub file_path: String,
    pub force: Option<bool>,
}

/// DTO for retrieving a group chat payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetGroupChatDto {
    pub id: String,
}

/// DTO for saving a group chat payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveGroupChatDto {
    pub id: String,
    pub chat: Vec<serde_json::Value>,
    pub force: Option<bool>,
}

/// DTO for saving a group chat payload from an existing JSONL file path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveGroupChatFromFileDto {
    pub id: String,
    pub file_path: String,
    pub force: Option<bool>,
}

/// DTO for deleting a group chat payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteGroupChatDto {
    pub id: String,
}

/// DTO for importing character chats from uploaded files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCharacterChatsDto {
    pub character_name: String,
    pub character_display_name: Option<String>,
    pub user_name: Option<String>,
    pub file_path: String,
    pub file_type: String,
}

/// DTO for importing group chats from uploaded files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportGroupChatDto {
    pub file_path: String,
}

/// DTO for renaming a group chat file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameGroupChatDto {
    pub old_file_name: String,
    pub new_file_name: String,
}

impl From<MessageExtra> for MessageExtraDto {
    fn from(extra: MessageExtra) -> Self {
        Self {
            api: extra.api,
            model: extra.model,
            reasoning: extra.reasoning,
            reasoning_duration: extra.reasoning_duration,
            token_count: extra.token_count,
            isSmallSys: extra.isSmallSys,
            gen_started: extra.gen_started,
            gen_finished: extra.gen_finished,
            swipe_id: extra.swipe_id,
            swipes: extra.swipes,
            swipe_info: extra.swipe_info,
            title: extra.title,
            force_avatar: extra.force_avatar,
            additional: extra.additional,
        }
    }
}

impl From<MessageExtraDto> for MessageExtra {
    fn from(dto: MessageExtraDto) -> Self {
        Self {
            api: dto.api,
            model: dto.model,
            reasoning: dto.reasoning,
            reasoning_duration: dto.reasoning_duration,
            token_count: dto.token_count,
            isSmallSys: dto.isSmallSys,
            gen_started: dto.gen_started,
            gen_finished: dto.gen_finished,
            swipe_id: dto.swipe_id,
            swipes: dto.swipes,
            swipe_info: dto.swipe_info,
            title: dto.title,
            force_avatar: dto.force_avatar,
            additional: dto.additional,
        }
    }
}

impl From<ChatMessage> for ChatMessageDto {
    fn from(message: ChatMessage) -> Self {
        Self {
            name: message.name,
            is_user: message.is_user,
            is_system: message.is_system,
            send_date: message.send_date,
            mes: message.mes,
            extra: MessageExtraDto::from(message.extra),
            additional: message.additional,
        }
    }
}

impl From<ChatMessageDto> for ChatMessage {
    fn from(dto: ChatMessageDto) -> Self {
        Self {
            name: dto.name,
            is_user: dto.is_user,
            is_system: dto.is_system,
            send_date: dto.send_date,
            mes: dto.mes,
            extra: MessageExtra::from(dto.extra),
            additional: dto.additional,
        }
    }
}

impl From<Chat> for ChatDto {
    fn from(chat: Chat) -> Self {
        let Chat {
            character_name,
            user_name,
            file_name,
            create_date,
            chat_metadata,
            messages,
        } = chat;

        let file_name =
            file_name.unwrap_or_else(|| format!("{} - {}", character_name, create_date));
        let chat_id = chat_metadata.chat_id_hash;
        let chat_metadata =
            serde_json::to_value(chat_metadata).unwrap_or_else(|_| serde_json::json!({}));
        let message_count = messages.len();
        let messages = messages.into_iter().map(ChatMessageDto::from).collect();

        Self {
            character_name,
            user_name,
            file_name,
            create_date,
            chat_metadata,
            messages,
            message_count,
            chat_id,
        }
    }
}

impl From<ChatSearchResult> for ChatSearchResultDto {
    fn from(result: ChatSearchResult) -> Self {
        Self {
            character_name: result.character_name,
            file_name: result.file_name,
            file_size: result.file_size,
            message_count: result.message_count,
            preview: result.preview,
            date: result.date,
            chat_id: result.chat_id,
            chat_metadata: result.chat_metadata,
        }
    }
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

impl From<String> for ChatExportFormat {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "plaintext" => ChatExportFormat::PlainText,
            _ => ChatExportFormat::JSONL,
        }
    }
}
