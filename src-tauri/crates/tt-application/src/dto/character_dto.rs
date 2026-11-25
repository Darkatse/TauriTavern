use super::avatar_image_dto::ImageCropDto;
use chrono::{SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tt_domain::models::character::Character;
use tt_ports::repositories::character_repository::{
    CharacterChat, CharacterCreateResult, CharacterCreateWarning,
};

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
    pub data_size: u64,
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
    pub card_json: String,
    pub file_name: Option<String>,
    pub primary_lorebook: Option<String>,
}

/// Raw character card update DTO used by upstream-compatible HTTP routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCharacterCardDataDto {
    pub card_json: String,
    pub avatar_path: Option<String>,
    pub crop: Option<ImageCropDto>,
    #[serde(default)]
    pub materialize_primary_lorebook: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptCharacterLorebookDto {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CharacterLorebookAdoptionDto {
    None,
    Linked {
        world: String,
        previous: String,
        created: bool,
        updated: bool,
    },
    Decision {
        world: String,
        target: String,
        can_update: bool,
        token: String,
    },
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CharacterLorebookAdoptionChoice {
    Adopt,
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveCharacterLorebookAdoptionDto {
    pub name: String,
    pub choice: CharacterLorebookAdoptionChoice,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCharacterResultDto {
    pub character: CharacterDto,
    pub lorebook: CharacterLorebookAdoptionDto,
}

/// Raw character card merge DTO used by upstream-compatible HTTP routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeCharacterCardDataDto {
    pub update_json: String,
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
    pub data_json: String,
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

/// Existing character replacement DTO. `name` is the exact storage stem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceCharacterDto {
    pub file_path: String,
    pub name: String,
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
            data_size,
            date_added,
            date_last_chat,
            json_data,
            data,
            ..
        } = character;

        let create_date = if create_date.trim().is_empty() && date_added > 0 {
            format_timestamp_millis(date_added).unwrap_or(create_date)
        } else {
            create_date
        };

        let extensions = if shallow {
            Some(serde_json::json!({
                "talkativeness": data.extensions.talkativeness(),
                "fav": data.extensions.fav(),
                "world": data.extensions.world(),
            }))
        } else {
            Some(
                serde_json::to_value(&data.extensions)
                    .expect("CharacterExtensions serialization should not fail"),
            )
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
            data_size,
            date_added,
            date_last_chat,
            alternate_greetings: data.alternate_greetings,
            system_prompt: data.system_prompt,
            post_history_instructions: data.post_history_instructions,
            extensions,
            character_book: data.character_book,
            json_data,
        }
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

#[cfg(test)]
mod tests {
    use super::CharacterDto;
    use chrono::{SecondsFormat, TimeZone, Utc};
    use tt_domain::models::character::Character;

    #[test]
    fn character_dto_falls_back_to_date_added_when_create_date_missing() {
        let character = Character {
            date_added: 1_700_000_000_123,
            ..Default::default()
        };

        let dto = CharacterDto::from(character);
        let expected = Utc
            .timestamp_millis_opt(1_700_000_000_123)
            .single()
            .expect("valid timestamp")
            .to_rfc3339_opts(SecondsFormat::Millis, true);

        assert_eq!(dto.create_date, expected);
    }
}
