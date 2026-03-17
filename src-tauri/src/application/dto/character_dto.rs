use crate::domain::models::character::{Character, CharacterExtensions};
use crate::domain::repositories::character_repository::{CharacterChat, ImageCrop};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Character response DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterDto {
    pub shallow: bool,
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
    pub character_book: Option<serde_json::Value>,
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
    pub chat: Option<String>,
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

/// Character export content DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCharacterContentDto {
    pub name: String,
    pub format: String,
}

/// Character export content response DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCharacterContentResultDto {
    pub data: Vec<u8>,
    pub mime_type: String,
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
            shallow: character.shallow,
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
            character_book: character.data.character_book.clone(),
        }
    }
}

fn replace_character_extensions(
    character: &mut Character,
    extensions: Option<Value>,
) -> Result<(), serde_json::Error> {
    if let Some(extensions) = extensions {
        character.data.extensions = serde_json::from_value::<CharacterExtensions>(extensions)?;
    }

    Ok(())
}

pub(crate) fn merge_character_extensions(
    character: &mut Character,
    extensions: Value,
) -> Result<(), serde_json::Error> {
    let mut current = serde_json::to_value(&character.data.extensions)?;
    merge_json_value(&mut current, extensions);
    character.data.extensions = serde_json::from_value::<CharacterExtensions>(current)?;
    Ok(())
}

fn merge_json_value(current: &mut Value, updates: Value) {
    match (current, updates) {
        (Value::Object(current_object), Value::Object(updates_object)) => {
            for (key, value) in updates_object {
                match current_object.get_mut(&key) {
                    Some(current_value) => merge_json_value(current_value, value),
                    None => {
                        current_object.insert(key, value);
                    }
                }
            }
        }
        (current_value, updates_value) => *current_value = updates_value,
    }
}

/// Convert from DTO to domain model
impl TryFrom<CreateCharacterDto> for Character {
    type Error = serde_json::Error;

    fn try_from(dto: CreateCharacterDto) -> Result<Self, Self::Error> {
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
        replace_character_extensions(&mut character, dto.extensions)?;
        character.data.extensions.talkativeness = character.talkativeness;
        character.data.extensions.fav = character.fav;

        Ok(character)
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

#[cfg(test)]
mod tests {
    use super::{CreateCharacterDto, merge_character_extensions};
    use crate::domain::models::character::Character;
    use serde_json::json;

    #[test]
    fn try_from_create_character_dto_maps_structured_extensions() {
        let character = Character::try_from(CreateCharacterDto {
            name: "Test".to_string(),
            description: "desc".to_string(),
            personality: "persona".to_string(),
            scenario: String::new(),
            first_mes: "hello".to_string(),
            mes_example: String::new(),
            creator: None,
            creator_notes: None,
            character_version: None,
            tags: None,
            talkativeness: Some(0.75),
            fav: Some(true),
            alternate_greetings: None,
            system_prompt: None,
            post_history_instructions: None,
            extensions: Some(json!({
                "world": "bound-book",
                "depth_prompt": {
                    "prompt": "focus",
                    "depth": 7,
                    "role": "assistant"
                },
                "custom": "value"
            })),
        })
        .expect("character conversion should succeed");

        assert_eq!(character.data.extensions.world, "bound-book");
        assert_eq!(character.data.extensions.depth_prompt.prompt, "focus");
        assert_eq!(character.data.extensions.depth_prompt.depth, 7);
        assert_eq!(character.data.extensions.depth_prompt.role, "assistant");
        assert_eq!(
            character.data.extensions.additional.get("custom"),
            Some(&json!("value"))
        );
        assert_eq!(character.talkativeness, 0.75);
        assert!(character.fav);
        assert_eq!(character.data.extensions.talkativeness, 0.75);
        assert!(character.data.extensions.fav);
    }

    #[test]
    fn merge_character_extensions_preserves_existing_fields() {
        let mut character = Character::new(
            "Test".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hello".to_string(),
        );
        character.data.extensions.world = "existing".to_string();
        character
            .data
            .extensions
            .additional
            .insert("custom".to_string(), json!("old"));

        merge_character_extensions(
            &mut character,
            json!({
                "world": "",
                "fav": true,
                "custom_2": "new"
            }),
        )
        .expect("extensions merge should succeed");

        assert_eq!(character.data.extensions.world, "");
        assert!(character.data.extensions.fav);
        assert_eq!(
            character.data.extensions.additional.get("custom"),
            Some(&json!("old"))
        );
        assert_eq!(
            character.data.extensions.additional.get("custom_2"),
            Some(&json!("new"))
        );
    }

    #[test]
    fn merge_character_extensions_preserves_nested_fields() {
        let mut character = Character::new(
            "Test".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hello".to_string(),
        );
        character.data.extensions.depth_prompt.prompt = "old".to_string();
        character.data.extensions.depth_prompt.depth = 7;
        character.data.extensions.depth_prompt.role = "assistant".to_string();

        merge_character_extensions(
            &mut character,
            json!({
                "depth_prompt": {
                    "prompt": "new"
                }
            }),
        )
        .expect("extensions merge should succeed");

        assert_eq!(character.data.extensions.depth_prompt.prompt, "new");
        assert_eq!(character.data.extensions.depth_prompt.depth, 7);
        assert_eq!(character.data.extensions.depth_prompt.role, "assistant");
    }
}
