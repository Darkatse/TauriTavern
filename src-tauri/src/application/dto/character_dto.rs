use crate::domain::json_merge::merge_json_value;
use crate::domain::models::character::{Character, CharacterExtensions};
use crate::domain::repositories::character_repository::{
    CharacterChat, CharacterCreateResult, CharacterCreateWarning, ImageCrop,
};
use chrono::{SecondsFormat, TimeZone, Utc};
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
    pub talkativeness: f64,
    pub fav: bool,
    pub chat_size: u64,
    pub date_added: i64,
    pub date_last_chat: i64,
    pub alternate_greetings: Vec<String>,
    pub system_prompt: String,
    pub post_history_instructions: String,
    pub extensions: Option<serde_json::Value>,
    pub character_book: Option<serde_json::Value>,
    pub json_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterCreateWarningDto {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCharacterWithAvatarResultDto {
    pub character: CharacterDto,
    pub warnings: Vec<CharacterCreateWarningDto>,
}

fn format_timestamp_millis(timestamp_millis: i64) -> Option<String> {
    Utc.timestamp_millis_opt(timestamp_millis)
        .single()
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
}

impl From<CharacterCreateWarning> for CharacterCreateWarningDto {
    fn from(warning: CharacterCreateWarning) -> Self {
        Self {
            code: warning.code,
            message: warning.message,
        }
    }
}

impl From<CharacterCreateResult> for CreateCharacterWithAvatarResultDto {
    fn from(result: CharacterCreateResult) -> Self {
        Self {
            character: CharacterDto::from(result.character),
            warnings: result.warnings.into_iter().map(Into::into).collect(),
        }
    }
}

/// Character creation DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCharacterDto {
    pub file_name: Option<String>,
    pub json_data: Option<String>,
    pub primary_lorebook: Option<String>,
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
    pub talkativeness: Option<f64>,
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
    pub talkativeness: Option<f64>,
    pub fav: Option<bool>,
    pub alternate_greetings: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub post_history_instructions: Option<String>,
    pub extensions: Option<serde_json::Value>,
}

/// Raw character card update DTO used by upstream-compatible HTTP routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCharacterCardDataDto {
    pub card_json: String,
    pub avatar_path: Option<String>,
    pub crop: Option<ImageCropDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckCharacterLorebookConflictDto {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterLorebookConflictDto {
    pub conflict: bool,
    pub world: String,
    pub embedded_name: Option<String>,
    pub current_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CharacterLorebookConflictResolution {
    Current,
    Embedded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveCharacterLorebookConflictDto {
    pub name: String,
    pub resolution: CharacterLorebookConflictResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveCharacterLorebookConflictResultDto {
    pub world: String,
}

/// Raw character card merge DTO used by upstream-compatible HTTP routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeCharacterCardDataDto {
    pub update: serde_json::Value,
}

/// Bulk character card merge filter DTO used by upstream-compatible HTTP routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkMergeCharacterCardDataFilterDto {
    pub path: String,
}

/// Bulk character card merge DTO used by upstream-compatible HTTP routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkMergeCharacterCardDataDto {
    #[serde(default)]
    pub avatars: Vec<String>,
    pub data: serde_json::Value,
    pub filter: Option<BulkMergeCharacterCardDataFilterDto>,
}

/// Bulk character card merge result DTO.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BulkMergeCharacterCardDataResultDto {
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<String>,
}

/// Character rename DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameCharacterDto {
    pub old_name: String,
    pub new_name: String,
}

/// Character duplicate DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCharacterDto {
    pub name: String,
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
        let Character {
            shallow,
            name,
            description,
            personality,
            scenario,
            first_mes,
            mes_example,
            avatar,
            chat,
            creator,
            creator_notes,
            character_version,
            tags,
            create_date,
            talkativeness,
            fav,
            chat_size,
            date_added,
            date_last_chat,
            data,
            ..
        } = character;

        let create_date = if create_date.trim().is_empty() && date_added > 0 {
            format_timestamp_millis(date_added).unwrap_or(create_date)
        } else {
            create_date
        };

        let extensions = if shallow {
            None
        } else {
            Some(serde_json::to_value(&data.extensions).unwrap_or(serde_json::Value::Null))
        };

        Self {
            shallow,
            name,
            description,
            personality,
            scenario,
            first_mes,
            mes_example,
            avatar,
            chat,
            creator,
            creator_notes,
            character_version,
            tags,
            create_date,
            talkativeness,
            fav,
            chat_size,
            date_added,
            date_last_chat,
            alternate_greetings: data.alternate_greetings,
            system_prompt: data.system_prompt,
            post_history_instructions: data.post_history_instructions,
            extensions,
            character_book: data.character_book,
            json_data: None,
        }
    }
}

impl CharacterDto {
    pub fn with_json_data(mut self, json_data: Option<String>) -> Self {
        self.json_data = json_data;
        self
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

/// Convert from DTO to domain model
impl TryFrom<CreateCharacterDto> for Character {
    type Error = serde_json::Error;

    fn try_from(dto: CreateCharacterDto) -> Result<Self, Self::Error> {
        let file_name = dto
            .file_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.strip_suffix(".png").unwrap_or(value).to_string());
        let mut character =
            Character::new(dto.name, dto.description, dto.personality, dto.first_mes);
        character.file_name = file_name;
        character.json_data = dto.json_data;

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
    use super::{CharacterDto, CreateCharacterDto, merge_character_extensions};
    use crate::domain::models::character::Character;
    use chrono::{SecondsFormat, TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn try_from_create_character_dto_maps_structured_extensions() {
        let character = Character::try_from(CreateCharacterDto {
            file_name: None,
            json_data: None,
            primary_lorebook: None,
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
        assert_eq!(character.file_name, None);
        assert_eq!(character.talkativeness, 0.75);
        assert!(character.fav);
        assert_eq!(character.data.extensions.talkativeness, 0.75);
        assert!(character.data.extensions.fav);
    }

    #[test]
    fn try_from_create_character_dto_preserves_explicit_file_name() {
        let character = Character::try_from(CreateCharacterDto {
            file_name: Some("Assistant.png".to_string()),
            json_data: None,
            primary_lorebook: None,
            name: "Assistant".to_string(),
            description: String::new(),
            personality: String::new(),
            scenario: String::new(),
            first_mes: String::new(),
            mes_example: String::new(),
            creator: None,
            creator_notes: None,
            character_version: None,
            tags: None,
            talkativeness: None,
            fav: None,
            alternate_greetings: None,
            system_prompt: None,
            post_history_instructions: None,
            extensions: None,
        })
        .expect("character conversion should succeed");

        assert_eq!(character.file_name, Some("Assistant".to_string()));
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

    #[test]
    fn character_dto_falls_back_to_date_added_when_create_date_missing() {
        let mut character = Character::new(
            "Fallback".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hi".to_string(),
        );

        character.create_date = "".to_string();
        character.date_added = 1_700_000_000_123;

        let dto = CharacterDto::from(character);
        let expected = Utc
            .timestamp_millis_opt(1_700_000_000_123)
            .single()
            .expect("valid timestamp")
            .to_rfc3339_opts(SecondsFormat::Millis, true);

        assert_eq!(dto.create_date, expected);
    }

    #[test]
    fn character_dto_preserves_existing_create_date() {
        let mut character = Character::new(
            "Preserve".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hi".to_string(),
        );

        character.create_date = "2026-03-18T12:34:56.789Z".to_string();
        character.date_added = 1_700_000_000_123;

        let dto = CharacterDto::from(character);
        assert_eq!(dto.create_date, "2026-03-18T12:34:56.789Z");
    }
}
