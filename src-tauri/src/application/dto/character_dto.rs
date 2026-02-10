use crate::domain::models::character::Character;
use crate::domain::repositories::character_repository::{CharacterChat, ImageCrop};
use serde::{Deserialize, Serialize};

/// Character response DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterDto {
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub first_mes: String,
    pub mes_example: String,
    pub avatar: String,
    pub chat: String,
    pub creator: String,
    pub creator_notes: String,
    pub character_version: String,
    pub tags: Vec<String>,
    pub create_date: String,
    pub talkativeness: f32,
    pub fav: bool,
    pub chat_size: u64,
    pub date_added: i64,
    pub date_last_chat: i64,
    pub alternate_greetings: Vec<String>,
    pub system_prompt: String,
    pub post_history_instructions: String,
    pub extensions: Option<serde_json::Value>,
}

/// Character creation DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCharacterDto {
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub first_mes: String,
    pub mes_example: String,
    pub creator: Option<String>,
    pub creator_notes: Option<String>,
    pub character_version: Option<String>,
    pub tags: Option<Vec<String>>,
    pub talkativeness: Option<f32>,
    pub fav: Option<bool>,
    pub alternate_greetings: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub post_history_instructions: Option<String>,
    pub extensions: Option<serde_json::Value>,
}

/// Character update DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCharacterDto {
    pub name: Option<String>,
    pub description: Option<String>,
    pub personality: Option<String>,
    pub scenario: Option<String>,
    pub first_mes: Option<String>,
    pub mes_example: Option<String>,
    pub creator: Option<String>,
    pub creator_notes: Option<String>,
    pub character_version: Option<String>,
    pub tags: Option<Vec<String>>,
    pub talkativeness: Option<f32>,
    pub fav: Option<bool>,
    pub alternate_greetings: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub post_history_instructions: Option<String>,
    pub extensions: Option<serde_json::Value>,
}

/// Character rename DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameCharacterDto {
    pub old_name: String,
    pub new_name: String,
}

/// Character import DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCharacterDto {
    pub file_path: String,
    pub preserve_file_name: Option<String>,
}

/// Character export DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCharacterDto {
    pub name: String,
    pub target_path: String,
}

/// Character avatar update DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAvatarDto {
    pub name: String,
    pub avatar_path: String,
    pub crop: Option<ImageCropDto>,
}

/// Character creation with avatar DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWithAvatarDto {
    pub character: CreateCharacterDto,
    pub avatar_path: Option<String>,
    pub crop: Option<ImageCropDto>,
}

/// Image crop DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCropDto {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub want_resize: bool,
}

/// Character chat DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterChatDto {
    pub file_name: String,
    pub file_size: String,
    pub chat_items: usize,
    pub last_message: String,
    pub last_message_date: i64,
}

/// Character delete DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteCharacterDto {
    pub name: String,
    pub delete_chats: bool,
}

/// Character chats request DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCharacterChatsDto {
    pub name: String,
    pub simple: bool,
}

/// Convert from domain model to DTO
impl From<Character> for CharacterDto {
    fn from(character: Character) -> Self {
        Self {
            name: character.name,
            description: character.description,
            personality: character.personality,
            scenario: character.scenario,
            first_mes: character.first_mes,
            mes_example: character.mes_example,
            avatar: character.avatar,
            chat: character.chat,
            creator: character.creator,
            creator_notes: character.creator_notes,
            character_version: character.character_version,
            tags: character.tags,
            create_date: character.create_date,
            talkativeness: character.talkativeness,
            fav: character.fav,
            chat_size: character.chat_size,
            date_added: character.date_added,
            date_last_chat: character.date_last_chat,
            alternate_greetings: character.data.alternate_greetings.clone(),
            system_prompt: character.data.system_prompt.clone(),
            post_history_instructions: character.data.post_history_instructions.clone(),
            extensions: Some(
                serde_json::to_value(&character.data.extensions).unwrap_or(serde_json::Value::Null),
            ),
        }
    }
}

/// Convert from DTO to domain model
impl From<CreateCharacterDto> for Character {
    fn from(dto: CreateCharacterDto) -> Self {
        let mut character =
            Character::new(dto.name, dto.description, dto.personality, dto.first_mes);

        character.scenario = dto.scenario;
        character.mes_example = dto.mes_example;
        character.creator = dto.creator.unwrap_or_default();
        character.creator_notes = dto.creator_notes.unwrap_or_default();
        character.character_version = dto.character_version.unwrap_or_default();
        character.tags = dto.tags.unwrap_or_default();
        character.talkativeness = dto.talkativeness.unwrap_or(0.5);
        character.fav = dto.fav.unwrap_or(false);

        // Update data fields
        character.data.scenario = character.scenario.clone();
        character.data.mes_example = character.mes_example.clone();
        character.data.creator = character.creator.clone();
        character.data.creator_notes = character.creator_notes.clone();
        character.data.character_version = character.character_version.clone();
        character.data.tags = character.tags.clone();
        character.data.alternate_greetings = dto.alternate_greetings.unwrap_or_default();
        character.data.system_prompt = dto.system_prompt.unwrap_or_default();
        character.data.post_history_instructions =
            dto.post_history_instructions.unwrap_or_default();
        character.data.extensions.talkativeness = character.talkativeness;
        character.data.extensions.fav = character.fav;

        // Set extensions if provided
        if let Some(extensions) = dto.extensions {
            if let Ok(ext_map) =
                serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(extensions)
            {
                for (key, value) in ext_map {
                    character.data.extensions.additional.insert(key, value);
                }
            }
        }

        character
    }
}

/// Convert from domain model to DTO
impl From<CharacterChat> for CharacterChatDto {
    fn from(chat: CharacterChat) -> Self {
        Self {
            file_name: chat.file_name,
            file_size: chat.file_size,
            chat_items: chat.chat_items,
            last_message: chat.last_message,
            last_message_date: chat.last_message_date,
        }
    }
}

/// Convert from DTO to domain model
impl From<ImageCropDto> for ImageCrop {
    fn from(dto: ImageCropDto) -> Self {
        Self {
            x: dto.x,
            y: dto.y,
            width: dto.width,
            height: dto.height,
            want_resize: dto.want_resize,
        }
    }
}
