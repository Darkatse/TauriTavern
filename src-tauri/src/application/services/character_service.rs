use crate::application::dto::character_dto::{
    CharacterChatDto, CharacterDto, CreateCharacterDto, CreateWithAvatarDto, DeleteCharacterDto,
    ExportCharacterContentDto, ExportCharacterContentResultDto, ExportCharacterDto,
    GetCharacterChatsDto, ImportCharacterDto, RenameCharacterDto, UpdateAvatarDto,
    UpdateCharacterDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::character::Character;
use crate::domain::models::world_info::sanitize_world_info_name;
use crate::domain::repositories::character_repository::{CharacterRepository, ImageCrop};
use crate::domain::repositories::world_info_repository::WorldInfoRepository;
use crate::infrastructure::logging::logger;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// Service for character management
pub struct CharacterService {
    repository: Arc<dyn CharacterRepository>,
    world_info_repository: Arc<dyn WorldInfoRepository>,
}

impl CharacterService {
    /// Create a new CharacterService
    pub fn new(
        repository: Arc<dyn CharacterRepository>,
        world_info_repository: Arc<dyn WorldInfoRepository>,
    ) -> Self {
        Self {
            repository,
            world_info_repository,
        }
    }

    /// Get all characters
    pub async fn get_all_characters(
        &self,
        shallow: bool,
    ) -> Result<Vec<CharacterDto>, ApplicationError> {
        logger::debug("Getting all characters");
        let characters = self.repository.find_all(shallow).await?;
        Ok(characters.into_iter().map(CharacterDto::from).collect())
    }

    /// Get a character by name
    pub async fn get_character(&self, name: &str) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!("Getting character: {}", name));
        let character = self.repository.find_by_name(name).await?;
        Ok(CharacterDto::from(character))
    }

    /// Create a new character
    pub async fn create_character(
        &self,
        dto: CreateCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!("Creating character: {}", dto.name));

        // Convert DTO to domain model
        let character = Character::from(dto);

        // Validate character
        self.validate_character(&character)?;

        // Save character
        self.repository.save(&character).await?;

        Ok(CharacterDto::from(character))
    }

    /// Create a character with an avatar
    pub async fn create_with_avatar(
        &self,
        dto: CreateWithAvatarDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!(
            "Creating character with avatar: {}",
            dto.character.name
        ));

        // Convert DTO to domain model
        let character = Character::from(dto.character);

        // Validate character
        self.validate_character(&character)?;

        // Convert avatar path
        let avatar_path_ref: Option<&Path> = dto.avatar_path.as_deref().map(Path::new);

        // Convert crop parameters
        let crop = dto.crop.map(|c_dto| ImageCrop::from(c_dto));

        // Create character with avatar
        let created = self
            .repository
            .create_with_avatar(&character, avatar_path_ref, crop)
            .await?;

        Ok(CharacterDto::from(created))
    }

    /// Update a character
    pub async fn update_character(
        &self,
        name: &str,
        dto: UpdateCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!("Updating character: {}", name));

        // Get the existing character
        let mut character = self.repository.find_by_name(name).await?;

        // Apply updates
        if let Some(new_name) = dto.name {
            character.name = new_name;
            character.data.name = character.name.clone();
        }

        if let Some(chat) = dto.chat {
            character.chat = chat;
        }

        if let Some(description) = dto.description {
            character.description = description;
            character.data.description = character.description.clone();
        }

        if let Some(personality) = dto.personality {
            character.personality = personality;
            character.data.personality = character.personality.clone();
        }

        if let Some(scenario) = dto.scenario {
            character.scenario = scenario;
            character.data.scenario = character.scenario.clone();
        }

        if let Some(first_mes) = dto.first_mes {
            character.first_mes = first_mes;
            character.data.first_mes = character.first_mes.clone();
        }

        if let Some(mes_example) = dto.mes_example {
            character.mes_example = mes_example;
            character.data.mes_example = character.mes_example.clone();
        }

        if let Some(creator) = dto.creator {
            character.creator = creator;
            character.data.creator = character.creator.clone();
        }

        if let Some(creator_notes) = dto.creator_notes {
            character.creator_notes = creator_notes;
            character.data.creator_notes = character.creator_notes.clone();
        }

        if let Some(character_version) = dto.character_version {
            character.character_version = character_version;
            character.data.character_version = character.character_version.clone();
        }

        if let Some(tags) = dto.tags {
            character.tags = tags;
            character.data.tags = character.tags.clone();
        }

        if let Some(talkativeness) = dto.talkativeness {
            character.talkativeness = talkativeness;
            character.data.extensions.talkativeness = character.talkativeness;
        }

        if let Some(fav) = dto.fav {
            character.fav = fav;
            character.data.extensions.fav = character.fav;
        }

        if let Some(alternate_greetings) = dto.alternate_greetings {
            character.data.alternate_greetings = alternate_greetings;
        }

        if let Some(system_prompt) = dto.system_prompt {
            character.data.system_prompt = system_prompt;
        }

        if let Some(post_history_instructions) = dto.post_history_instructions {
            character.data.post_history_instructions = post_history_instructions;
        }

        if let Some(extensions) = dto.extensions {
            if let Ok(ext_map) =
                serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(extensions)
            {
                for (key, value) in ext_map {
                    character.data.extensions.additional.insert(key, value);
                }
            }
        }

        // Validate character
        self.validate_character(&character)?;

        // Save character
        self.repository.update(&character).await?;

        Ok(CharacterDto::from(character))
    }

    /// Delete a character
    pub async fn delete_character(&self, dto: DeleteCharacterDto) -> Result<(), ApplicationError> {
        logger::debug(&format!("Deleting character: {}", dto.name));
        self.repository.delete(&dto.name, dto.delete_chats).await?;
        Ok(())
    }

    /// Rename a character
    pub async fn rename_character(
        &self,
        dto: RenameCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!(
            "Renaming character: {} -> {}",
            dto.old_name, dto.new_name
        ));
        let character = self.repository.rename(&dto.old_name, &dto.new_name).await?;
        Ok(CharacterDto::from(character))
    }

    /// Import a character
    pub async fn import_character(
        &self,
        dto: ImportCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!("Importing character from: {}", dto.file_path));
        let mut character = self
            .repository
            .import_character(Path::new(&dto.file_path), dto.preserve_file_name)
            .await?;

        self.try_auto_import_embedded_world_info(&mut character)
            .await?;

        Ok(CharacterDto::from(character))
    }

    /// Export a character
    pub async fn export_character(&self, dto: ExportCharacterDto) -> Result<(), ApplicationError> {
        logger::debug(&format!(
            "Exporting character: {} to {}",
            dto.name, dto.target_path
        ));
        self.repository
            .export_character(&dto.name, Path::new(&dto.target_path))
            .await?;
        Ok(())
    }

    /// Export character as downloadable content (PNG/JSON)
    pub async fn export_character_content(
        &self,
        dto: ExportCharacterContentDto,
    ) -> Result<ExportCharacterContentResultDto, ApplicationError> {
        let format = dto.format.trim().to_ascii_lowercase();
        if format != "png" && format != "json" {
            return Err(ApplicationError::ValidationError(format!(
                "Unsupported character export format: {}",
                dto.format
            )));
        }

        let character = self.repository.find_by_name(&dto.name).await?;
        let export_value = Self::build_export_card_value(&character)?;

        if format == "json" {
            let pretty_json = serde_json::to_string_pretty(&export_value).map_err(|error| {
                ApplicationError::InternalError(format!(
                    "Failed to serialize exported character JSON: {}",
                    error
                ))
            })?;

            return Ok(ExportCharacterContentResultDto {
                data: pretty_json.into_bytes(),
                mime_type: "application/json".to_string(),
            });
        }

        let card_json = serde_json::to_string(&export_value).map_err(|error| {
            ApplicationError::InternalError(format!(
                "Failed to serialize exported character card JSON: {}",
                error
            ))
        })?;

        let png_bytes = self
            .repository
            .export_character_png_bytes(&dto.name, &card_json)
            .await?;

        Ok(ExportCharacterContentResultDto {
            data: png_bytes,
            mime_type: "image/png".to_string(),
        })
    }

    /// Update a character's avatar
    pub async fn update_avatar(&self, dto: UpdateAvatarDto) -> Result<(), ApplicationError> {
        logger::debug(&format!("Updating avatar for character: {}", dto.name));
        let crop = dto.crop.map(|c_dto| ImageCrop::from(c_dto));
        self.repository
            .update_avatar(&dto.name, Path::new(&dto.avatar_path), crop)
            .await?;
        Ok(())
    }

    /// Get character chats
    pub async fn get_character_chats(
        &self,
        dto: GetCharacterChatsDto,
    ) -> Result<Vec<CharacterChatDto>, ApplicationError> {
        logger::debug(&format!("Getting chats for character: {}", dto.name));
        let chats = self
            .repository
            .get_character_chats(&dto.name, dto.simple)
            .await?;
        Ok(chats.into_iter().map(CharacterChatDto::from).collect())
    }

    /// Clear the character cache
    pub async fn clear_cache(&self) -> Result<(), ApplicationError> {
        logger::debug("Clearing character cache");
        self.repository.clear_cache().await?;
        Ok(())
    }

    /// Validate a character
    fn validate_character(&self, character: &Character) -> Result<(), DomainError> {
        // Check required fields
        if character.name.trim().is_empty() {
            return Err(DomainError::InvalidData(
                "Character name is required".to_string(),
            ));
        }

        // Check name length
        if character.name.len() > 100 {
            return Err(DomainError::InvalidData(
                "Character name is too long (max 100 characters)".to_string(),
            ));
        }

        Ok(())
    }

    async fn try_auto_import_embedded_world_info(
        &self,
        character: &mut Character,
    ) -> Result<(), DomainError> {
        let Some(character_book) = character.data.character_book.clone() else {
            return Ok(());
        };

        let converted_world = match Self::convert_character_book_to_world_info(&character_book) {
            Ok(value) => value,
            Err(error) => {
                logger::warn(&format!(
                    "Skipping embedded world info import for {}: {}",
                    character.name, error
                ));
                return Ok(());
            }
        };

        let preferred_name = Self::resolve_embedded_world_name(character, &character_book);
        let world_name = self
            .resolve_available_world_name(&preferred_name, &converted_world)
            .await?;

        self.world_info_repository
            .save_world_info(&world_name, &converted_world)
            .await?;

        if character.data.extensions.world != world_name {
            character.data.extensions.world = world_name;
            self.repository.update(character).await?;
        }

        Ok(())
    }

    fn resolve_embedded_world_name(character: &Character, character_book: &Value) -> String {
        if !character.data.extensions.world.trim().is_empty() {
            return character.data.extensions.world.trim().to_string();
        }

        if let Some(book_name) = character_book.get("name").and_then(Value::as_str) {
            let trimmed = book_name.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }

        format!("{}'s Lorebook", character.name)
    }

    async fn resolve_available_world_name(
        &self,
        preferred_name: &str,
        payload: &Value,
    ) -> Result<String, DomainError> {
        let base_name = sanitize_world_info_name(preferred_name);
        if base_name.is_empty() {
            return Err(DomainError::InvalidData(
                "Embedded world info name is invalid".to_string(),
            ));
        }

        let existing = self
            .world_info_repository
            .get_world_info(&base_name, false)
            .await?;

        if let Some(existing_payload) = existing {
            if existing_payload == *payload {
                return Ok(base_name);
            }

            let names: HashSet<String> = self
                .world_info_repository
                .list_world_names()
                .await?
                .into_iter()
                .collect();

            let mut suffix = 2usize;
            loop {
                let candidate = sanitize_world_info_name(&format!("{} {}", base_name, suffix));
                if !candidate.is_empty() && !names.contains(&candidate) {
                    return Ok(candidate);
                }
                suffix += 1;
            }
        }

        Ok(base_name)
    }

    fn convert_character_book_to_world_info(character_book: &Value) -> Result<Value, DomainError> {
        if let Some(entries_object) = character_book.get("entries").and_then(Value::as_object) {
            return Ok(json!({
                "entries": entries_object,
                "originalData": character_book,
            }));
        }

        let entries = character_book
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                DomainError::InvalidData("Embedded character book has invalid entries".to_string())
            })?;

        let mut converted_entries = Map::new();
        for (index, entry) in entries.iter().enumerate() {
            let converted_entry = Self::convert_character_book_entry(entry, index);
            let uid = converted_entry
                .get("uid")
                .and_then(Value::as_i64)
                .unwrap_or(index as i64);
            converted_entries.insert(uid.to_string(), converted_entry);
        }

        Ok(json!({
            "entries": converted_entries,
            "originalData": character_book,
        }))
    }

    fn convert_character_book_entry(entry: &Value, index: usize) -> Value {
        let id = entry
            .get("id")
            .and_then(Value::as_i64)
            .unwrap_or(index as i64);
        let comment = entry
            .get("comment")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let position = entry
            .pointer("/extensions/position")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| {
                if entry.get("position").and_then(Value::as_str) == Some("before_char") {
                    0
                } else {
                    1
                }
            });
        let enabled = entry
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let extensions = entry
            .get("extensions")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let mut result = Map::new();
        result.insert("uid".to_string(), json!(id));
        result.insert(
            "key".to_string(),
            json!(Self::parse_string_array(entry.get("keys"))),
        );
        result.insert(
            "keysecondary".to_string(),
            json!(Self::parse_string_array(entry.get("secondary_keys"))),
        );
        result.insert("comment".to_string(), json!(comment));
        result.insert(
            "content".to_string(),
            json!(entry.get("content").and_then(Value::as_str).unwrap_or("")),
        );
        result.insert(
            "constant".to_string(),
            json!(entry
                .get("constant")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "selective".to_string(),
            json!(entry
                .get("selective")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "order".to_string(),
            json!(entry
                .get("insertion_order")
                .and_then(Value::as_i64)
                .unwrap_or(100)),
        );
        result.insert("position".to_string(), json!(position));
        result.insert(
            "excludeRecursion".to_string(),
            json!(entry
                .pointer("/extensions/exclude_recursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "preventRecursion".to_string(),
            json!(entry
                .pointer("/extensions/prevent_recursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "delayUntilRecursion".to_string(),
            json!(entry
                .pointer("/extensions/delay_until_recursion")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert("disable".to_string(), json!(!enabled));
        result.insert("addMemo".to_string(), json!(!comment.is_empty()));
        result.insert(
            "displayIndex".to_string(),
            json!(entry
                .pointer("/extensions/display_index")
                .and_then(Value::as_i64)
                .unwrap_or(index as i64)),
        );
        result.insert(
            "probability".to_string(),
            entry
                .pointer("/extensions/probability")
                .cloned()
                .unwrap_or_else(|| json!(100)),
        );
        result.insert(
            "useProbability".to_string(),
            json!(entry
                .pointer("/extensions/useProbability")
                .and_then(Value::as_bool)
                .unwrap_or(true)),
        );
        result.insert(
            "depth".to_string(),
            json!(entry
                .pointer("/extensions/depth")
                .and_then(Value::as_i64)
                .unwrap_or(4)),
        );
        result.insert(
            "selectiveLogic".to_string(),
            json!(entry
                .pointer("/extensions/selectiveLogic")
                .and_then(Value::as_i64)
                .unwrap_or(0)),
        );
        result.insert(
            "outletName".to_string(),
            json!(entry
                .pointer("/extensions/outlet_name")
                .and_then(Value::as_str)
                .unwrap_or("")),
        );
        result.insert(
            "group".to_string(),
            json!(entry
                .pointer("/extensions/group")
                .and_then(Value::as_str)
                .unwrap_or("")),
        );
        result.insert(
            "groupOverride".to_string(),
            json!(entry
                .pointer("/extensions/group_override")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "groupWeight".to_string(),
            json!(entry
                .pointer("/extensions/group_weight")
                .and_then(Value::as_i64)
                .unwrap_or(100)),
        );
        result.insert(
            "scanDepth".to_string(),
            entry
                .pointer("/extensions/scan_depth")
                .cloned()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "caseSensitive".to_string(),
            entry
                .pointer("/extensions/case_sensitive")
                .cloned()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "matchWholeWords".to_string(),
            entry
                .pointer("/extensions/match_whole_words")
                .cloned()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "useGroupScoring".to_string(),
            entry
                .pointer("/extensions/use_group_scoring")
                .cloned()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "automationId".to_string(),
            json!(entry
                .pointer("/extensions/automation_id")
                .and_then(Value::as_str)
                .unwrap_or("")),
        );
        result.insert(
            "role".to_string(),
            json!(entry
                .pointer("/extensions/role")
                .and_then(Value::as_i64)
                .unwrap_or(0)),
        );
        result.insert(
            "vectorized".to_string(),
            json!(entry
                .pointer("/extensions/vectorized")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "sticky".to_string(),
            entry
                .pointer("/extensions/sticky")
                .cloned()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "cooldown".to_string(),
            entry
                .pointer("/extensions/cooldown")
                .cloned()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "delay".to_string(),
            entry
                .pointer("/extensions/delay")
                .cloned()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "matchPersonaDescription".to_string(),
            json!(entry
                .pointer("/extensions/match_persona_description")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "matchCharacterDescription".to_string(),
            json!(entry
                .pointer("/extensions/match_character_description")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "matchCharacterPersonality".to_string(),
            json!(entry
                .pointer("/extensions/match_character_personality")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "matchCharacterDepthPrompt".to_string(),
            json!(entry
                .pointer("/extensions/match_character_depth_prompt")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "matchScenario".to_string(),
            json!(entry
                .pointer("/extensions/match_scenario")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert(
            "matchCreatorNotes".to_string(),
            json!(entry
                .pointer("/extensions/match_creator_notes")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        result.insert("extensions".to_string(), Value::Object(extensions));
        result.insert(
            "triggers".to_string(),
            entry
                .pointer("/extensions/triggers")
                .cloned()
                .unwrap_or_else(|| json!([])),
        );
        result.insert(
            "ignoreBudget".to_string(),
            json!(entry
                .pointer("/extensions/ignore_budget")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );

        Value::Object(result)
    }

    fn parse_string_array(value: Option<&Value>) -> Vec<String> {
        match value {
            Some(Value::Array(values)) => values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect(),
            Some(Value::String(values)) => values
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect(),
            _ => Vec::new(),
        }
    }

    fn build_export_card_value(character: &Character) -> Result<Value, DomainError> {
        let mut export_card = character.to_v2();
        export_card.fav = false;
        export_card.data.extensions.fav = false;

        let mut export_value = serde_json::to_value(&export_card).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to build exported character payload: {}",
                error
            ))
        })?;

        if let Some(object) = export_value.as_object_mut() {
            object.remove("chat");
        }

        Ok(export_value)
    }
}

#[cfg(test)]
mod tests {
    use super::CharacterService;
    use crate::domain::models::character::Character;
    use serde_json::json;

    #[test]
    fn convert_character_book_builds_world_info_structure() {
        let character_book = json!({
            "name": "Lore",
            "entries": [
                {
                    "id": 42,
                    "keys": ["alpha", "beta"],
                    "secondary_keys": ["gamma"],
                    "comment": "memo",
                    "content": "content",
                    "constant": true,
                    "selective": false,
                    "insertion_order": 150,
                    "enabled": true,
                    "position": "before_char",
                    "extensions": {
                        "position": 0,
                        "useProbability": true,
                        "depth": 6,
                        "triggers": ["normal"]
                    }
                }
            ]
        });

        let converted = CharacterService::convert_character_book_to_world_info(&character_book)
            .expect("conversion should succeed");
        let entries = converted
            .get("entries")
            .and_then(|value| value.as_object())
            .expect("entries object");
        let entry = entries
            .get("42")
            .and_then(|value| value.as_object())
            .expect("entry by id");

        assert_eq!(entry.get("uid").and_then(|value| value.as_i64()), Some(42));
        assert_eq!(
            entry
                .get("key")
                .and_then(|value| value.as_array())
                .map(|value| value.len()),
            Some(2)
        );
        assert_eq!(
            entry
                .get("keysecondary")
                .and_then(|value| value.as_array())
                .map(|value| value.len()),
            Some(1)
        );
        assert_eq!(
            entry.get("position").and_then(|value| value.as_i64()),
            Some(0)
        );
        assert_eq!(
            entry.get("disable").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            converted.get("originalData"),
            Some(&character_book),
            "original character_book should be preserved"
        );
    }

    #[test]
    fn parse_string_array_accepts_array_and_csv() {
        let from_array = CharacterService::parse_string_array(Some(&json!(["a", " b ", ""])));
        let from_csv = CharacterService::parse_string_array(Some(&json!("x, y , ,z")));

        assert_eq!(from_array, vec!["a", "b"]);
        assert_eq!(from_csv, vec!["x", "y", "z"]);
    }

    #[test]
    fn build_export_card_value_removes_private_fields() {
        let mut character = Character::new(
            "Export Test".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hello".to_string(),
        );
        character.chat = "private-chat-name".to_string();
        character.fav = true;
        character.data.extensions.fav = true;

        let export_value = CharacterService::build_export_card_value(&character)
            .expect("export payload should be built");

        assert!(
            export_value.get("chat").is_none(),
            "chat should be removed from exported payload"
        );
        assert_eq!(
            export_value.get("fav").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            export_value
                .pointer("/data/extensions/fav")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
    }
}
