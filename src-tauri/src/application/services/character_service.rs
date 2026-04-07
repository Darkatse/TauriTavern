mod card_contract;
mod lorebook_codec;

use crate::application::dto::character_dto::{
    CharacterChatDto, CharacterDto, CreateCharacterDto, CreateWithAvatarDto, DeleteCharacterDto,
    ExportCharacterContentDto, ExportCharacterContentResultDto, ExportCharacterDto,
    GetCharacterChatsDto, ImportCharacterDto, MergeCharacterCardDataDto, RenameCharacterDto,
    UpdateAvatarDto, UpdateCharacterCardDataDto, UpdateCharacterDto, merge_character_extensions,
};
use crate::application::errors::ApplicationError;
use crate::domain::json_merge::merge_json_value;
use crate::domain::errors::DomainError;
use crate::domain::models::character::Character;
use crate::domain::models::world_info::sanitize_world_info_name;
use crate::domain::repositories::character_repository::{CharacterRepository, ImageCrop};
use crate::domain::repositories::world_info_repository::WorldInfoRepository;
use crate::infrastructure::logging::logger;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use self::lorebook_codec::{character_book_to_world_info, world_info_to_character_book};

/// Service for character management
pub struct CharacterService {
    repository: Arc<dyn CharacterRepository>,
    world_info_repository: Arc<dyn WorldInfoRepository>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CharacterCardValidationMode {
    ReadableOnly,
    Strict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CharacterCardLorebookMaterializationMode {
    MaterializePrimary,
    Skip,
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
        let raw_json = character.json_data.clone();
        Ok(CharacterDto::from(character).with_json_data(raw_json))
    }

    /// Create a new character
    pub async fn create_character(
        &self,
        dto: CreateCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!("Creating character: {}", dto.name));

        // Convert DTO to domain model
        let mut character = Character::try_from(dto).map_err(Self::map_extensions_error)?;

        // Validate character
        self.validate_character(&character)?;
        self.materialize_primary_lorebook(&mut character).await?;

        let created = self
            .repository
            .create_with_avatar(&character, None, None)
            .await?;

        Ok(CharacterDto::from(created))
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
        let mut character =
            Character::try_from(dto.character).map_err(Self::map_extensions_error)?;

        // Validate character
        self.validate_character(&character)?;
        self.materialize_primary_lorebook(&mut character).await?;

        // Convert avatar path
        let avatar_path_ref: Option<&Path> = dto.avatar_path.as_deref().map(Path::new);

        // Convert crop parameters
        let crop = dto.crop.map(ImageCrop::from);

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
        let raw_json = match character.json_data.clone() {
            Some(value) => value,
            None => self.repository.read_character_card_json(name).await?,
        };
        let mut card_value = card_contract::parse_character_card_json(&raw_json)?;
        let UpdateCharacterDto {
            name: new_name,
            chat,
            description,
            personality,
            scenario,
            first_mes,
            mes_example,
            creator,
            creator_notes,
            character_version,
            tags,
            talkativeness,
            fav,
            alternate_greetings,
            system_prompt,
            post_history_instructions,
            extensions,
        } = dto;

        // Apply updates
        if let Some(new_name) = new_name {
            character.name = new_name;
            character.data.name = character.name.clone();
        }

        if let Some(chat) = chat {
            character.chat = chat;
        }

        if let Some(description) = description {
            character.description = description;
            character.data.description = character.description.clone();
        }

        if let Some(personality) = personality {
            character.personality = personality;
            character.data.personality = character.personality.clone();
        }

        if let Some(scenario) = scenario {
            character.scenario = scenario;
            character.data.scenario = character.scenario.clone();
        }

        if let Some(first_mes) = first_mes {
            character.first_mes = first_mes;
            character.data.first_mes = character.first_mes.clone();
        }

        if let Some(mes_example) = mes_example {
            character.mes_example = mes_example;
            character.data.mes_example = character.mes_example.clone();
        }

        if let Some(creator) = creator {
            character.creator = creator;
            character.data.creator = character.creator.clone();
        }

        if let Some(creator_notes) = creator_notes {
            character.creator_notes = creator_notes;
            character.data.creator_notes = character.creator_notes.clone();
        }

        if let Some(character_version) = character_version {
            character.character_version = character_version;
            character.data.character_version = character.character_version.clone();
        }

        if let Some(tags) = tags {
            character.tags = tags;
            character.data.tags = character.tags.clone();
        }

        if let Some(talkativeness) = talkativeness {
            character.talkativeness = talkativeness;
            character.data.extensions.talkativeness = character.talkativeness;
        }

        if let Some(fav) = fav {
            character.fav = fav;
            character.data.extensions.fav = character.fav;
        }

        if let Some(alternate_greetings) = alternate_greetings {
            character.data.alternate_greetings = alternate_greetings;
        }

        if let Some(system_prompt) = system_prompt {
            character.data.system_prompt = system_prompt;
        }

        if let Some(post_history_instructions) = post_history_instructions {
            character.data.post_history_instructions = post_history_instructions;
        }

        if let Some(extensions) = extensions {
            merge_character_extensions(&mut character, extensions)
                .map_err(Self::map_extensions_error)?;
        }

        if talkativeness.is_some() {
            character.data.extensions.talkativeness = character.talkativeness;
        } else {
            character.talkativeness = character.data.extensions.talkativeness;
        }

        if fav.is_some() {
            character.data.extensions.fav = character.fav;
        } else {
            character.fav = character.data.extensions.fav;
        }

        let updated_value = serde_json::to_value(&character.to_v2()).map_err(|error| {
            ApplicationError::InternalError(format!(
                "Failed to serialize updated character payload: {}",
                error
            ))
        })?;
        merge_json_value(&mut card_value, updated_value);

        let updated = self
            .write_character_card_value(
                name,
                card_value,
                None,
                None,
                CharacterCardValidationMode::ReadableOnly,
                CharacterCardLorebookMaterializationMode::MaterializePrimary,
            )
            .await?;

        Ok(CharacterDto::from(updated))
    }

    /// Update a character card using upstream-compatible raw card JSON semantics.
    pub async fn update_character_card_data(
        &self,
        name: &str,
        dto: UpdateCharacterCardDataDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!("Updating character card data: {}", name));

        let crop = dto.crop.map(ImageCrop::from);
        let avatar_path = dto.avatar_path.as_deref().map(Path::new);
        let updated = self
            .write_character_card_value(
                name,
                card_contract::parse_character_card_json(&dto.card_json)?,
                avatar_path,
                crop,
                CharacterCardValidationMode::ReadableOnly,
                CharacterCardLorebookMaterializationMode::MaterializePrimary,
            )
            .await?;

        Ok(CharacterDto::from(updated))
    }

    /// Merge raw attributes into a stored character card using upstream-compatible deep merge semantics.
    pub async fn merge_character_card_data(
        &self,
        name: &str,
        dto: MergeCharacterCardDataDto,
    ) -> Result<CharacterDto, ApplicationError> {
        logger::debug(&format!("Merging character card data: {}", name));

        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut card_value = card_contract::parse_character_card_json(&raw_json)?;
        merge_json_value(&mut card_value, dto.update);
        let updated = self
            .write_character_card_value(
                name,
                card_value,
                None,
                None,
                CharacterCardValidationMode::Strict,
                CharacterCardLorebookMaterializationMode::Skip,
            )
            .await?;

        Ok(CharacterDto::from(updated))
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
        self.validate_character_name(&dto.new_name)?;

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
        let export_value = self.build_export_card_value(&dto.name).await?;
        let export_json = serde_json::to_string_pretty(&export_value).map_err(|error| {
            ApplicationError::InternalError(format!(
                "Failed to serialize exported character JSON: {}",
                error
            ))
        })?;

        self.repository
            .export_character(&dto.name, Path::new(&dto.target_path), &export_json)
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

        let export_value = self.build_export_card_value(&dto.name).await?;

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
        let mut character = self.repository.find_by_name(&dto.name).await?;
        self.materialize_primary_lorebook(&mut character).await?;

        let crop = dto.crop.map(ImageCrop::from);
        self.repository
            .update_avatar(&character, Path::new(&dto.avatar_path), crop)
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
    pub async fn clear_cache(&self) -> Result<(), DomainError> {
        logger::debug("Clearing character cache");
        self.repository.clear_cache().await
    }

    /// Validate a character
    fn validate_character(&self, character: &Character) -> Result<(), DomainError> {
        self.validate_character_name(&character.name)
    }

    fn validate_character_name(&self, name: &str) -> Result<(), DomainError> {
        if name.trim().is_empty() {
            return Err(DomainError::InvalidData(
                "Character name is required".to_string(),
            ));
        }

        Ok(())
    }

    fn map_extensions_error(error: serde_json::Error) -> ApplicationError {
        ApplicationError::ValidationError(format!("Invalid character extensions: {}", error))
    }

    async fn write_character_card_value(
        &self,
        name: &str,
        mut card_value: Value,
        avatar_path: Option<&Path>,
        crop: Option<ImageCrop>,
        validation_mode: CharacterCardValidationMode,
        lorebook_mode: CharacterCardLorebookMaterializationMode,
    ) -> Result<Character, ApplicationError> {
        let card_json = self
            .prepare_character_card_json_for_write(&mut card_value, validation_mode, lorebook_mode)
            .await?;

        self.repository
            .write_character_card_json(name, &card_json, avatar_path, crop)
            .await
            .map_err(Into::into)
    }

    async fn prepare_character_card_json_for_write(
        &self,
        card_value: &mut Value,
        validation_mode: CharacterCardValidationMode,
        lorebook_mode: CharacterCardLorebookMaterializationMode,
    ) -> Result<String, ApplicationError> {
        card_contract::strip_character_card_json_data(card_value);
        if lorebook_mode == CharacterCardLorebookMaterializationMode::MaterializePrimary {
            self.materialize_primary_lorebook_value(card_value).await?;
        }
        card_contract::normalize_v2_character_book_extensions(card_value)?;
        self.validate_character_card_for_write(card_value, validation_mode)?;

        serde_json::to_string(card_value).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "Failed to serialize character card payload: {}",
                error
            ))
        })
    }

    fn validate_character_card_for_write(
        &self,
        card_value: &Value,
        validation_mode: CharacterCardValidationMode,
    ) -> Result<(), ApplicationError> {
        match validation_mode {
            CharacterCardValidationMode::ReadableOnly => {
                let name = card_contract::character_card_name(card_value)?;
                self.validate_character_name(name)?;
                card_contract::ensure_readable_character_card(card_value)
            }
            CharacterCardValidationMode::Strict => {
                self.validate_character_card_value(card_value)?;
                card_contract::ensure_readable_character_card(card_value)
            }
        }
    }

    fn validate_character_card_value(&self, card_value: &Value) -> Result<(), DomainError> {
        card_contract::validate_character_card_schema(card_value)?;
        let name = card_contract::character_card_name(card_value)?;
        self.validate_character_name(name)
    }

    async fn materialize_primary_lorebook(
        &self,
        character: &mut Character,
    ) -> Result<bool, DomainError> {
        let world_name = character.data.extensions.world.trim();
        if world_name.is_empty() {
            let removed = character.data.character_book.take().is_some();
            return Ok(removed);
        }

        let world_info = self
            .world_info_repository
            .get_world_info(world_name, false)
            .await?
            .ok_or_else(|| {
                DomainError::NotFound(format!("World info file {} doesn't exist", world_name))
            })?;
        let character_book = world_info_to_character_book(world_name, &world_info)?;

        if character.data.character_book.as_ref() == Some(&character_book) {
            return Ok(false);
        }

        character.data.character_book = Some(character_book);
        Ok(true)
    }

    async fn try_auto_import_embedded_world_info(
        &self,
        character: &mut Character,
    ) -> Result<(), DomainError> {
        let Some(character_book) = character.data.character_book.clone() else {
            return Ok(());
        };

        let converted_world = match character_book_to_world_info(&character_book) {
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

    async fn build_export_card_value(&self, name: &str) -> Result<Value, DomainError> {
        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut export_value: Value = serde_json::from_str(&raw_json).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to parse stored character payload: {}",
                error
            ))
        })?;

        self.materialize_primary_lorebook_value(&mut export_value)
            .await?;
        card_contract::normalize_v2_character_book_extensions(&mut export_value)?;
        card_contract::unset_private_fields(&mut export_value)?;

        Ok(export_value)
    }

    async fn materialize_primary_lorebook_value(
        &self,
        export_value: &mut Value,
    ) -> Result<(), DomainError> {
        let world_name = export_value
            .pointer("/data/extensions/world")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();

        if world_name.is_empty() {
            if let Some(data_object) = export_value.get_mut("data").and_then(Value::as_object_mut) {
                data_object.remove("character_book");
            }
            return Ok(());
        }

        let world_info = self
            .world_info_repository
            .get_world_info(world_name, false)
            .await?
            .ok_or_else(|| {
                DomainError::NotFound(format!("World info file {} doesn't exist", world_name))
            })?;
        let character_book = world_info_to_character_book(world_name, &world_info)?;

        let Some(root_object) = export_value.as_object_mut() else {
            return Err(DomainError::InvalidData(
                "Character payload must be a JSON object".to_string(),
            ));
        };

        let data = root_object
            .entry("data")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(data_object) = data.as_object_mut() else {
            return Err(DomainError::InvalidData(
                "Character payload data must be a JSON object".to_string(),
            ));
        };

        data_object.insert("character_book".to_string(), character_book);

        Ok(())
    }
}

#[cfg(test)]
mod tests;
