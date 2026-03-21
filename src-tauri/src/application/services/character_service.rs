mod lorebook_codec;

use crate::application::dto::character_dto::{
    CharacterChatDto, CharacterDto, CreateCharacterDto, CreateWithAvatarDto, DeleteCharacterDto,
    ExportCharacterContentDto, ExportCharacterContentResultDto, ExportCharacterDto,
    GetCharacterChatsDto, ImportCharacterDto, MergeCharacterCardDataDto, RenameCharacterDto,
    UpdateAvatarDto, UpdateCharacterCardDataDto, UpdateCharacterDto, merge_character_extensions,
    merge_json_value,
};
use crate::application::errors::ApplicationError;
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

        // Validate character
        self.validate_character(&character)?;
        self.materialize_primary_lorebook(&mut character).await?;

        // Save character
        self.repository.update(&character).await?;

        Ok(CharacterDto::from(character))
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
                Self::parse_character_card_json(&dto.card_json)?,
                avatar_path,
                crop,
                CharacterCardValidationMode::ReadableOnly,
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
        let mut card_value = Self::parse_character_card_json(&raw_json)?;
        merge_json_value(&mut card_value, dto.update);
        let updated = self
            .write_character_card_value(
                name,
                card_value,
                None,
                None,
                CharacterCardValidationMode::Strict,
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
    ) -> Result<Character, ApplicationError> {
        let card_json = self
            .prepare_character_card_json_for_write(&mut card_value, validation_mode)
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
    ) -> Result<String, ApplicationError> {
        Self::strip_character_card_json_data(card_value);
        self.materialize_primary_lorebook_value(card_value).await?;
        Self::normalize_v2_character_book_extensions(card_value)?;
        self.validate_character_card_for_write(card_value, validation_mode)?;

        serde_json::to_string(card_value).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "Failed to serialize character card payload: {}",
                error
            ))
        })
    }

    fn parse_character_card_json(card_json: &str) -> Result<Value, ApplicationError> {
        let value: Value = serde_json::from_str(card_json).map_err(|error| {
            ApplicationError::ValidationError(format!("Invalid character card JSON: {}", error))
        })?;

        if !value.is_object() {
            return Err(ApplicationError::ValidationError(
                "Character card payload must be a JSON object".to_string(),
            ));
        }

        Ok(value)
    }

    fn strip_character_card_json_data(card_value: &mut Value) {
        if let Some(card_object) = card_value.as_object_mut() {
            card_object.remove("json_data");
        }
    }

    fn ensure_readable_character_card(card_value: &Value) -> Result<(), ApplicationError> {
        serde_json::from_value::<Character>(card_value.clone()).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "Character card payload is not readable: {}",
                error
            ))
        })?;
        Ok(())
    }

    fn normalize_v2_character_book_extensions(card_value: &mut Value) -> Result<(), DomainError> {
        if card_value.get("spec").and_then(Value::as_str) != Some("chara_card_v2") {
            return Ok(());
        }

        let Some(character_book) = card_value.pointer_mut("/data/character_book") else {
            return Ok(());
        };
        let Some(character_book_object) = character_book.as_object_mut() else {
            return Err(Self::invalid_character_card_field("data.character_book"));
        };

        match character_book_object.get("extensions") {
            Some(Value::Object(_)) => Ok(()),
            Some(_) => Err(Self::invalid_character_card_field(
                "data.character_book.extensions",
            )),
            None => {
                character_book_object.insert(
                    "extensions".to_string(),
                    Value::Object(serde_json::Map::new()),
                );
                Ok(())
            }
        }
    }

    fn validate_character_card_for_write(
        &self,
        card_value: &Value,
        validation_mode: CharacterCardValidationMode,
    ) -> Result<(), ApplicationError> {
        match validation_mode {
            CharacterCardValidationMode::ReadableOnly => {
                let name = Self::character_card_name(card_value)?;
                self.validate_character_name(name)?;
                Self::ensure_readable_character_card(card_value)
            }
            CharacterCardValidationMode::Strict => {
                self.validate_character_card_value(card_value)?;
                Self::ensure_readable_character_card(card_value)
            }
        }
    }

    fn validate_character_card_value(&self, card_value: &Value) -> Result<(), DomainError> {
        match card_value.get("spec").and_then(Value::as_str) {
            Some("chara_card_v2") => Self::validate_v2_character_card(card_value)?,
            Some("chara_card_v3") => Self::validate_v3_character_card(card_value)?,
            Some(_) => return Err(Self::invalid_character_card_field("spec")),
            None => Self::validate_v1_character_card(card_value)?,
        }

        let name = Self::character_card_name(card_value)?;
        self.validate_character_name(name)
    }

    fn character_card_name(card_value: &Value) -> Result<&str, DomainError> {
        card_value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                card_value
                    .pointer("/data/name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .ok_or_else(|| Self::missing_character_card_field("name"))
    }

    fn validate_v1_character_card(card_value: &Value) -> Result<(), DomainError> {
        for field in [
            "name",
            "description",
            "personality",
            "scenario",
            "first_mes",
            "mes_example",
        ] {
            if card_value.get(field).is_none() {
                return Err(Self::missing_character_card_field(field));
            }
        }

        Ok(())
    }

    fn validate_v2_character_card(card_value: &Value) -> Result<(), DomainError> {
        if card_value.get("spec_version").and_then(Value::as_str) != Some("2.0") {
            return Err(Self::invalid_character_card_field("spec_version"));
        }

        let Some(data) = card_value.get("data").and_then(Value::as_object) else {
            return Err(Self::missing_character_card_field("data"));
        };

        for field in [
            "name",
            "description",
            "personality",
            "scenario",
            "first_mes",
            "mes_example",
            "creator_notes",
            "system_prompt",
            "post_history_instructions",
            "alternate_greetings",
            "tags",
            "creator",
            "character_version",
            "extensions",
        ] {
            if !data.contains_key(field) {
                return Err(Self::missing_character_card_field(&format!(
                    "data.{}",
                    field
                )));
            }
        }

        if !data.get("alternate_greetings").is_some_and(Value::is_array) {
            return Err(Self::invalid_character_card_field(
                "data.alternate_greetings",
            ));
        }

        if !data.get("tags").is_some_and(Value::is_array) {
            return Err(Self::invalid_character_card_field("data.tags"));
        }

        if !data.get("extensions").is_some_and(Value::is_object) {
            return Err(Self::invalid_character_card_field("data.extensions"));
        }

        if let Some(character_book) = data.get("character_book") {
            let Some(character_book) = character_book.as_object() else {
                return Err(Self::invalid_character_card_field("data.character_book"));
            };

            if !character_book.contains_key("extensions") {
                return Err(Self::missing_character_card_field(
                    "data.character_book.extensions",
                ));
            }

            if !character_book.contains_key("entries") {
                return Err(Self::missing_character_card_field(
                    "data.character_book.entries",
                ));
            }

            if !character_book
                .get("extensions")
                .is_some_and(Value::is_object)
            {
                return Err(Self::invalid_character_card_field(
                    "data.character_book.extensions",
                ));
            }

            if !character_book.get("entries").is_some_and(Value::is_array) {
                return Err(Self::invalid_character_card_field(
                    "data.character_book.entries",
                ));
            }
        }

        Ok(())
    }

    fn validate_v3_character_card(card_value: &Value) -> Result<(), DomainError> {
        let spec_version = card_value
            .get("spec_version")
            .and_then(Self::character_card_spec_version);

        if !spec_version.is_some_and(|value| (3.0..4.0).contains(&value)) {
            return Err(Self::invalid_character_card_field("spec_version"));
        }

        if !card_value.get("data").is_some_and(Value::is_object) {
            return Err(Self::missing_character_card_field("data"));
        }

        Ok(())
    }

    fn character_card_spec_version(value: &Value) -> Option<f64> {
        match value {
            Value::Number(number) => number.as_f64(),
            Value::String(string) => string.parse::<f64>().ok(),
            _ => None,
        }
    }

    fn missing_character_card_field(field: &str) -> DomainError {
        DomainError::InvalidData(format!("Character card field {} is required", field))
    }

    fn invalid_character_card_field(field: &str) -> DomainError {
        DomainError::InvalidData(format!("Character card field {} is invalid", field))
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
        Self::normalize_v2_character_book_extensions(&mut export_value)?;
        Self::unset_private_fields(&mut export_value)?;

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

    fn unset_private_fields(export_value: &mut Value) -> Result<(), DomainError> {
        let Some(root_object) = export_value.as_object_mut() else {
            return Err(DomainError::InvalidData(
                "Character payload must be a JSON object".to_string(),
            ));
        };

        root_object.insert("fav".to_string(), Value::Bool(false));
        root_object.remove("chat");

        let data = root_object
            .entry("data")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(data_object) = data.as_object_mut() else {
            return Err(DomainError::InvalidData(
                "Character payload data must be a JSON object".to_string(),
            ));
        };

        let extensions = data_object
            .entry("extensions")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(extensions_object) = extensions.as_object_mut() else {
            return Err(DomainError::InvalidData(
                "Character payload extensions must be a JSON object".to_string(),
            ));
        };

        extensions_object.insert("fav".to_string(), Value::Bool(false));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CharacterService;
    use crate::application::dto::character_dto::{
        CreateCharacterDto, ExportCharacterContentDto, ExportCharacterDto,
        MergeCharacterCardDataDto, UpdateAvatarDto, UpdateCharacterCardDataDto, UpdateCharacterDto,
    };
    use crate::application::errors::ApplicationError;
    use crate::domain::models::character::Character;
    use crate::domain::repositories::character_repository::CharacterRepository;
    use crate::domain::repositories::world_info_repository::WorldInfoRepository;
    use crate::infrastructure::persistence::png_utils::{
        read_character_data_from_png, write_character_data_to_png,
    };
    use crate::infrastructure::repositories::file_character_repository::FileCharacterRepository;
    use crate::infrastructure::repositories::file_world_info_repository::FileWorldInfoRepository;
    use image::{DynamicImage, ImageFormat, RgbaImage};
    use rand::random;
    use serde_json::json;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::fs;

    async fn write_character_png(root: &PathBuf, file_stem: &str, payload: &serde_json::Value) {
        let png_bytes = write_character_data_to_png(
            &build_minimal_png(),
            &serde_json::to_string(payload).expect("serialize card payload"),
        )
        .expect("embed card in png");
        fs::write(
            root.join("characters").join(format!("{}.png", file_stem)),
            png_bytes,
        )
        .await
        .expect("write character png");
    }

    fn unique_temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-character-service-{}", random::<u64>()))
    }

    fn build_minimal_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::new(1, 1));
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);
        image
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("should build png image");
        output
    }

    async fn setup_service() -> (
        CharacterService,
        FileCharacterRepository,
        FileWorldInfoRepository,
        PathBuf,
    ) {
        let root = unique_temp_root();
        let characters_dir = root.join("characters");
        let chats_dir = root.join("chats");
        let worlds_dir = root.join("worlds");
        let default_avatar = root.join("default.png");

        fs::create_dir_all(&characters_dir)
            .await
            .expect("create characters dir");
        fs::create_dir_all(&chats_dir)
            .await
            .expect("create chats dir");
        fs::create_dir_all(&worlds_dir)
            .await
            .expect("create worlds dir");
        fs::write(&default_avatar, build_minimal_png())
            .await
            .expect("write default avatar");

        let character_repository =
            FileCharacterRepository::new(characters_dir, chats_dir, default_avatar);
        let world_info_repository = FileWorldInfoRepository::new(worlds_dir);
        let service = CharacterService::new(
            Arc::new(FileCharacterRepository::new(
                root.join("characters"),
                root.join("chats"),
                root.join("default.png"),
            )),
            Arc::new(FileWorldInfoRepository::new(root.join("worlds"))),
        );

        (service, character_repository, world_info_repository, root)
    }

    async fn save_bound_world(
        world_info_repository: &FileWorldInfoRepository,
        world_name: &str,
    ) -> serde_json::Value {
        let embedded_book: serde_json::Value = serde_json::from_str(
            r#"{
                "name": "",
                "entries": [
                    {
                        "id": 1,
                        "keys": ["alpha"],
                        "secondary_keys": [],
                        "comment": "",
                        "content": "content",
                        "constant": false,
                        "selective": false,
                        "insertion_order": 100,
                        "enabled": true,
                        "position": "after_char",
                        "use_regex": true,
                        "extensions": {
                            "position": 1,
                            "display_index": 0,
                            "probability": 100,
                            "useProbability": false,
                            "depth": 4,
                            "selectiveLogic": 0,
                            "outlet_name": "",
                            "group": "",
                            "group_override": false,
                            "group_weight": null,
                            "prevent_recursion": false,
                            "delay_until_recursion": false,
                            "scan_depth": null,
                            "match_whole_words": null,
                            "use_group_scoring": false,
                            "case_sensitive": null,
                            "automation_id": "",
                            "role": 0,
                            "vectorized": false,
                            "sticky": null,
                            "cooldown": null,
                            "delay": null,
                            "match_persona_description": false,
                            "match_character_description": false,
                            "match_character_personality": false,
                            "match_character_depth_prompt": false,
                            "match_scenario": false,
                            "match_creator_notes": false,
                            "triggers": [],
                            "ignore_budget": false
                        }
                    }
                ]
            }"#,
        )
        .expect("parse embedded book");
        let embedded_book = match embedded_book {
            serde_json::Value::Object(mut object) => {
                object.insert("name".to_string(), json!(world_name));
                serde_json::Value::Object(object)
            }
            _ => unreachable!("embedded book should be an object"),
        };
        let world_payload: serde_json::Value = serde_json::from_str(
            r#"{
                "entries": {
                    "1": {
                        "uid": 1,
                        "key": ["alpha"],
                        "keysecondary": [],
                        "comment": "",
                        "content": "content",
                        "constant": false,
                        "selective": false,
                        "order": 100,
                        "position": 1,
                        "disable": false,
                        "extensions": {},
                        "displayIndex": 0,
                        "probability": 100,
                        "useProbability": false,
                        "depth": 4,
                        "selectiveLogic": 0,
                        "outletName": "",
                        "group": "",
                        "groupOverride": false,
                        "groupWeight": null,
                        "preventRecursion": false,
                        "delayUntilRecursion": false,
                        "scanDepth": null,
                        "matchWholeWords": null,
                        "useGroupScoring": false,
                        "caseSensitive": null,
                        "automationId": "",
                        "role": 0,
                        "vectorized": false,
                        "sticky": null,
                        "cooldown": null,
                        "delay": null,
                        "matchPersonaDescription": false,
                        "matchCharacterDescription": false,
                        "matchCharacterPersonality": false,
                        "matchCharacterDepthPrompt": false,
                        "matchScenario": false,
                        "matchCreatorNotes": false,
                        "triggers": [],
                        "ignoreBudget": false
                    }
                }
            }"#,
        )
        .expect("parse bound world");
        let world_payload = match world_payload {
            serde_json::Value::Object(mut object) => {
                object.insert("originalData".to_string(), embedded_book.clone());
                serde_json::Value::Object(object)
            }
            _ => unreachable!("world payload should be an object"),
        };
        world_info_repository
            .save_world_info(world_name, &world_payload)
            .await
            .expect("save world info");
        embedded_book
    }

    async fn save_world_with_stale_original_data(
        world_info_repository: &FileWorldInfoRepository,
        world_name: &str,
    ) -> serde_json::Value {
        let original_book = json!({
            "name": "Imported Lore",
            "description": "preserve me",
            "entries": [
                {
                    "id": 1,
                    "keys": ["alpha"],
                    "content": "stale",
                    "extensions": {}
                }
            ]
        });
        let world_payload: serde_json::Value = serde_json::from_str(
            r#"{
                "entries": {
                    "7": {
                        "uid": 7,
                        "key": ["beta"],
                        "keysecondary": [],
                        "comment": "memo",
                        "content": "fresh",
                        "constant": false,
                        "selective": false,
                        "order": 33,
                        "position": 1,
                        "disable": false,
                        "extensions": {
                            "custom": "value"
                        },
                        "displayIndex": 0,
                        "probability": 100,
                        "useProbability": false,
                        "depth": 4,
                        "selectiveLogic": 0,
                        "outletName": "",
                        "group": "",
                        "groupOverride": false,
                        "groupWeight": null,
                        "preventRecursion": false,
                        "delayUntilRecursion": false,
                        "scanDepth": null,
                        "matchWholeWords": null,
                        "useGroupScoring": false,
                        "caseSensitive": null,
                        "automationId": "",
                        "role": 0,
                        "vectorized": false,
                        "sticky": null,
                        "cooldown": null,
                        "delay": null,
                        "matchPersonaDescription": false,
                        "matchCharacterDescription": false,
                        "matchCharacterPersonality": false,
                        "matchCharacterDepthPrompt": false,
                        "matchScenario": false,
                        "matchCreatorNotes": false,
                        "triggers": [],
                        "ignoreBudget": false
                    }
                }
            }"#,
        )
        .expect("parse world payload");
        let world_payload = match world_payload {
            serde_json::Value::Object(mut object) => {
                object.insert("originalData".to_string(), original_book.clone());
                serde_json::Value::Object(object)
            }
            _ => unreachable!("world payload should be an object"),
        };
        world_info_repository
            .save_world_info(world_name, &world_payload)
            .await
            .expect("save world info");

        original_book
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

        let mut export_value =
            serde_json::to_value(&character.to_v2()).expect("build export payload");
        CharacterService::unset_private_fields(&mut export_value)
            .expect("private fields should be removed");

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

    #[tokio::test]
    async fn export_character_content_preserves_unknown_card_fields() {
        let (service, _character_repository, _world_info_repository, root) = setup_service().await;

        let card_payload = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Unknown Export",
            "first_mes": "Hello",
            "creatorcomment": "legacy field",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Unknown Export",
                "first_mes": "Hello",
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system",
                    },
                },
                "x_custom_data": 123,
            },
        });

        write_character_png(&root, "Unknown Export", &card_payload).await;

        let exported = service
            .export_character_content(ExportCharacterContentDto {
                name: "Unknown Export".to_string(),
                format: "json".to_string(),
            })
            .await
            .expect("export should succeed");

        let exported_json = String::from_utf8(exported.data).expect("export json utf8");
        let exported_value: serde_json::Value =
            serde_json::from_str(&exported_json).expect("parse exported json");

        assert!(
            exported_value.get("x_custom_top").is_some(),
            "exported json should preserve unknown top-level fields"
        );
        assert!(
            exported_value.pointer("/data/x_custom_data").is_some(),
            "exported json should preserve unknown data fields"
        );

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn get_character_includes_raw_json_data() {
        let (service, _character_repository, _world_info_repository, root) = setup_service().await;

        let card_payload = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Raw Json Character",
            "first_mes": "Hello",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Raw Json Character",
                "first_mes": "Hello",
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    }
                },
                "x_custom_data": 123
            }
        });

        write_character_png(&root, "Raw Json Character", &card_payload).await;

        let dto = service
            .get_character("Raw Json Character")
            .await
            .expect("get character");
        let raw_json = dto.json_data.expect("character should include raw json");
        let raw_value: serde_json::Value = serde_json::from_str(&raw_json).expect("parse raw json");

        assert!(raw_value.get("x_custom_top").is_some());
        assert!(raw_value.pointer("/data/x_custom_data").is_some());

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn update_character_card_data_preserves_unknown_fields() {
        let (service, character_repository, _world_info_repository, root) = setup_service().await;

        let card_payload = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Raw Update",
            "description": "Before",
            "first_mes": "Hello",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Raw Update",
                "description": "Before",
                "first_mes": "Hello",
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    }
                },
                "x_custom_data": 123
            }
        });

        write_character_png(&root, "Raw Update", &card_payload).await;

        let updated_payload = json!({
            "spec": "chara_card_v2",
            "spec_version": "2.0",
            "name": "Raw Update",
            "description": "After",
            "personality": "",
            "scenario": "",
            "first_mes": "Hello",
            "mes_example": "",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Raw Update",
                "description": "After",
                "personality": "",
                "scenario": "",
                "first_mes": "Hello",
                "mes_example": "",
                "creator_notes": "",
                "system_prompt": "",
                "post_history_instructions": "",
                "tags": [],
                "creator": "",
                "character_version": "",
                "alternate_greetings": [],
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    },
                    "tavern_helper": {
                        "scripts": [
                            { "id": "script-1" }
                        ]
                    }
                },
                "x_custom_data": 123
            }
        });

        service
            .update_character_card_data(
                "Raw Update",
                UpdateCharacterCardDataDto {
                    card_json: serde_json::to_string(&updated_payload)
                        .expect("serialize update payload"),
                    avatar_path: None,
                    crop: None,
                },
            )
            .await
            .expect("update raw card data");

        let stored_json = character_repository
            .read_character_card_json("Raw Update")
            .await
            .expect("read updated character");
        let stored_value: serde_json::Value =
            serde_json::from_str(&stored_json).expect("parse updated character");

        assert!(stored_value.get("x_custom_top").is_some());
        assert!(stored_value.pointer("/data/x_custom_data").is_some());
        assert_eq!(
            stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
            Some(&json!("script-1"))
        );
        assert_eq!(
            stored_value
                .get("description")
                .and_then(serde_json::Value::as_str),
            Some("After")
        );

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn update_character_card_data_materializes_bound_lorebook_for_v3_origin_cards() {
        let (service, character_repository, world_info_repository, root) = setup_service().await;
        let embedded_book = save_bound_world(&world_info_repository, "bound-book").await;

        let source_card = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Bound Raw Update",
            "description": "Before",
            "first_mes": "Hello",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Bound Raw Update",
                "description": "Before",
                "first_mes": "Hello",
                "character_book": embedded_book,
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "bound-book",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    },
                    "tavern_helper": {
                        "scripts": [
                            { "id": "script-1" }
                        ]
                    }
                },
                "x_custom_data": 123
            }
        });
        write_character_png(&root, "Bound Raw Update", &source_card).await;

        let update_payload = json!({
            "spec": "chara_card_v2",
            "spec_version": "2.0",
            "name": "Bound Raw Update",
            "description": "After",
            "personality": "",
            "scenario": "",
            "first_mes": "Hello",
            "mes_example": "",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Bound Raw Update",
                "description": "After",
                "personality": "",
                "scenario": "",
                "first_mes": "Hello",
                "mes_example": "",
                "creator_notes": "",
                "system_prompt": "",
                "post_history_instructions": "",
                "tags": [],
                "creator": "",
                "character_version": "",
                "alternate_greetings": [],
                "character_book": {
                    "name": "bound-book",
                    "entries": [
                        {
                            "id": 1,
                            "keys": ["alpha"],
                            "content": "stale"
                        }
                    ]
                },
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "bound-book",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    },
                    "tavern_helper": {
                        "scripts": [
                            { "id": "script-1" }
                        ]
                    }
                },
                "x_custom_data": 123
            }
        });

        service
            .update_character_card_data(
                "Bound Raw Update",
                UpdateCharacterCardDataDto {
                    card_json: update_payload.to_string(),
                    avatar_path: None,
                    crop: None,
                },
            )
            .await
            .expect("bound world update should succeed");

        let stored_json = character_repository
            .read_character_card_json("Bound Raw Update")
            .await
            .expect("read updated character");
        let stored_value: serde_json::Value =
            serde_json::from_str(&stored_json).expect("parse updated character");
        assert_eq!(
            stored_value.pointer("/data/character_book/extensions"),
            Some(&json!({}))
        );
        assert_eq!(
            stored_value.pointer("/data/character_book/entries/0/content"),
            Some(&json!("content"))
        );
        assert_eq!(
            stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
            Some(&json!("script-1"))
        );

        let exported = service
            .export_character_content(ExportCharacterContentDto {
                name: "Bound Raw Update".to_string(),
                format: "json".to_string(),
            })
            .await
            .expect("export updated character");
        let exported_value: serde_json::Value =
            serde_json::from_slice(&exported.data).expect("parse exported character");
        assert_eq!(
            exported_value.pointer("/data/character_book/extensions"),
            Some(&json!({}))
        );
        assert_eq!(
            exported_value.pointer("/data/character_book/entries/0/content"),
            Some(&json!("content"))
        );
        assert_eq!(
            exported_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
            Some(&json!("script-1"))
        );
        assert!(exported_value.get("x_custom_top").is_some());
        assert!(exported_value.pointer("/data/x_custom_data").is_some());

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn merge_character_card_data_preserves_unknown_fields() {
        let (service, character_repository, _world_info_repository, root) = setup_service().await;

        let card_payload = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Raw Merge",
            "description": "Before",
            "first_mes": "Hello",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Raw Merge",
                "description": "Before",
                "first_mes": "Hello",
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    }
                },
                "x_custom_data": 123
            }
        });

        write_character_png(&root, "Raw Merge", &card_payload).await;

        service
            .merge_character_card_data(
                "Raw Merge",
                MergeCharacterCardDataDto {
                    update: json!({
                        "description": "After Merge",
                        "data": {
                            "extensions": {
                                "tavern_helper": {
                                    "scripts": [
                                        { "id": "merged-script" }
                                    ]
                                }
                            }
                        }
                    }),
                },
            )
            .await
            .expect("merge raw card data");

        let stored_json = character_repository
            .read_character_card_json("Raw Merge")
            .await
            .expect("read merged character");
        let stored_value: serde_json::Value =
            serde_json::from_str(&stored_json).expect("parse merged character");

        assert!(stored_value.get("x_custom_top").is_some());
        assert!(stored_value.pointer("/data/x_custom_data").is_some());
        assert_eq!(
            stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
            Some(&json!("merged-script"))
        );
        assert_eq!(
            stored_value
                .get("description")
                .and_then(serde_json::Value::as_str),
            Some("After Merge")
        );

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn merge_character_card_data_rejects_invalid_v2_payloads() {
        let (service, _character_repository, _world_info_repository, root) = setup_service().await;

        let card_payload = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Invalid Raw Merge",
            "description": "Before",
            "first_mes": "Hello",
            "data": {
                "name": "Invalid Raw Merge",
                "description": "Before",
                "first_mes": "Hello",
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    }
                }
            }
        });

        write_character_png(&root, "Invalid Raw Merge", &card_payload).await;

        let error = service
            .merge_character_card_data(
                "Invalid Raw Merge",
                MergeCharacterCardDataDto {
                    update: json!({
                        "spec": "chara_card_v2",
                        "spec_version": "2.0",
                        "description": "After",
                        "personality": "",
                        "scenario": "",
                        "mes_example": "",
                        "data": {
                            "name": "Invalid Raw Merge",
                            "description": "After",
                            "personality": "",
                            "scenario": "",
                            "first_mes": "Hello",
                            "mes_example": "",
                            "creator_notes": "",
                            "post_history_instructions": "",
                            "alternate_greetings": [],
                            "tags": [],
                            "creator": "",
                            "character_version": "",
                            "extensions": {}
                        }
                    }),
                },
            )
            .await
            .expect_err("invalid V2 payload should fail");

        assert!(matches!(error, ApplicationError::ValidationError(_)));
        assert!(
            error.to_string().contains("data.system_prompt"),
            "unexpected error: {}",
            error
        );

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn merge_character_card_data_succeeds_after_normal_bound_world_edit() {
        let (service, character_repository, world_info_repository, root) = setup_service().await;
        let embedded_book = save_bound_world(&world_info_repository, "bound-book").await;

        let source_card = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Bound Raw Merge",
            "description": "Before",
            "first_mes": "Hello",
            "data": {
                "name": "Bound Raw Merge",
                "description": "Before",
                "first_mes": "Hello",
                "character_book": embedded_book,
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": false,
                    "world": "bound-book",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system"
                    },
                    "tavern_helper": {
                        "scripts": [
                            { "id": "script-1" }
                        ]
                    }
                }
            }
        });
        write_character_png(&root, "Bound Raw Merge", &source_card).await;

        service
            .update_character_card_data(
                "Bound Raw Merge",
                UpdateCharacterCardDataDto {
                    card_json: json!({
                        "spec": "chara_card_v2",
                        "spec_version": "2.0",
                        "name": "Bound Raw Merge",
                        "description": "Before",
                        "personality": "",
                        "scenario": "",
                        "first_mes": "Hello",
                        "mes_example": "",
                        "data": {
                            "name": "Bound Raw Merge",
                            "description": "Before",
                            "personality": "",
                            "scenario": "",
                            "first_mes": "Hello",
                            "mes_example": "",
                            "creator_notes": "",
                            "system_prompt": "",
                            "post_history_instructions": "",
                            "tags": [],
                            "creator": "",
                            "character_version": "",
                            "alternate_greetings": [],
                            "character_book": {
                                "name": "bound-book",
                                "entries": [
                                    {
                                        "id": 1,
                                        "keys": ["alpha"],
                                        "content": "stale"
                                    }
                                ]
                            },
                            "extensions": {
                                "talkativeness": 0.5,
                                "fav": false,
                                "world": "bound-book",
                                "depth_prompt": {
                                    "prompt": "",
                                    "depth": 4,
                                    "role": "system"
                                },
                                "tavern_helper": {
                                    "scripts": [
                                        { "id": "script-1" }
                                    ]
                                }
                            }
                        }
                    })
                    .to_string(),
                    avatar_path: None,
                    crop: None,
                },
            )
            .await
            .expect("initial update should succeed");

        service
            .merge_character_card_data(
                "Bound Raw Merge",
                MergeCharacterCardDataDto {
                    update: json!({
                        "description": "After Merge"
                    }),
                },
            )
            .await
            .expect("merge after normal edit should succeed");

        let stored_json = character_repository
            .read_character_card_json("Bound Raw Merge")
            .await
            .expect("read merged character");
        let stored_value: serde_json::Value =
            serde_json::from_str(&stored_json).expect("parse merged character");
        assert_eq!(
            stored_value
                .get("description")
                .and_then(serde_json::Value::as_str),
            Some("After Merge")
        );
        assert_eq!(
            stored_value.pointer("/data/character_book/extensions"),
            Some(&json!({}))
        );
        assert_eq!(
            stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
            Some(&json!("script-1"))
        );

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn export_character_content_png_preserves_unknown_card_fields() {
        let (service, _character_repository, _world_info_repository, root) = setup_service().await;

        let card_payload = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "Unknown Export PNG",
            "first_mes": "Hello",
            "chat": "private-chat-name",
            "fav": true,
            "creatorcomment": "legacy field",
            "x_custom_top": { "nested": true },
            "data": {
                "name": "Unknown Export PNG",
                "first_mes": "Hello",
                "extensions": {
                    "talkativeness": 0.5,
                    "fav": true,
                    "world": "",
                    "depth_prompt": {
                        "prompt": "",
                        "depth": 4,
                        "role": "system",
                    },
                },
                "x_custom_data": 123,
            },
        });

        write_character_png(&root, "Unknown Export PNG", &card_payload).await;

        let exported = service
            .export_character_content(ExportCharacterContentDto {
                name: "Unknown Export PNG".to_string(),
                format: "png".to_string(),
            })
            .await
            .expect("export should succeed");

        let exported_json =
            read_character_data_from_png(&exported.data).expect("read exported png metadata");
        let exported_value: serde_json::Value =
            serde_json::from_str(&exported_json).expect("parse exported json");

        assert!(
            exported_value.get("x_custom_top").is_some(),
            "exported png should preserve unknown top-level fields"
        );
        assert!(
            exported_value.pointer("/data/x_custom_data").is_some(),
            "exported png should preserve unknown data fields"
        );
        assert!(
            exported_value.get("chat").is_none(),
            "chat should be removed from exported payload"
        );
        assert_eq!(
            exported_value.get("fav").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            exported_value
                .pointer("/data/extensions/fav")
                .and_then(|value| value.as_bool()),
            Some(false)
        );

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn create_character_persists_embedded_primary_lorebook() {
        let (service, character_repository, world_info_repository, root) = setup_service().await;
        save_bound_world(&world_info_repository, "bound-book").await;

        service
            .create_character(CreateCharacterDto {
                name: "Export Test".to_string(),
                description: "desc".to_string(),
                personality: "persona".to_string(),
                scenario: String::new(),
                first_mes: "hello".to_string(),
                mes_example: String::new(),
                creator: None,
                creator_notes: None,
                character_version: None,
                tags: None,
                talkativeness: Some(0.5),
                fav: Some(false),
                alternate_greetings: None,
                system_prompt: None,
                post_history_instructions: None,
                extensions: Some(json!({ "world": "bound-book" })),
            })
            .await
            .expect("create character");

        let stored = character_repository
            .find_by_name("Export Test")
            .await
            .expect("load stored character");
        assert_eq!(stored.data.extensions.world, "bound-book");
        assert_eq!(
            stored
                .data
                .character_book
                .as_ref()
                .and_then(|value| value.get("name")),
            Some(&json!("bound-book"))
        );
        assert_eq!(
            stored
                .data
                .character_book
                .as_ref()
                .and_then(|value| value.pointer("/entries/0/content")),
            Some(&json!("content"))
        );

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn create_character_requires_existing_primary_lorebook() {
        let (service, _character_repository, _world_info_repository, root) = setup_service().await;

        let error = service
            .create_character(CreateCharacterDto {
                name: "Missing World".to_string(),
                description: "desc".to_string(),
                personality: "persona".to_string(),
                scenario: String::new(),
                first_mes: "hello".to_string(),
                mes_example: String::new(),
                creator: None,
                creator_notes: None,
                character_version: None,
                tags: None,
                talkativeness: Some(0.5),
                fav: Some(false),
                alternate_greetings: None,
                system_prompt: None,
                post_history_instructions: None,
                extensions: Some(json!({ "world": "missing-book" })),
            })
            .await
            .expect_err("missing primary lorebook should fail");

        assert!(matches!(
            error,
            ApplicationError::NotFound(message) if message == "World info file missing-book doesn't exist"
        ));

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn export_character_content_materializes_bound_lorebook_for_stale_cards() {
        let (service, character_repository, world_info_repository, root) = setup_service().await;
        save_bound_world(&world_info_repository, "bound-book").await;

        let mut character = Character::new(
            "Stale Export".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hello".to_string(),
        );
        character.data.extensions.world = "bound-book".to_string();
        character_repository
            .save(&character)
            .await
            .expect("save stale character");

        let exported = service
            .export_character_content(ExportCharacterContentDto {
                name: "Stale Export".to_string(),
                format: "json".to_string(),
            })
            .await
            .expect("export character content");
        let export_value: serde_json::Value =
            serde_json::from_slice(&exported.data).expect("parse export json");

        assert_eq!(
            export_value.pointer("/data/character_book/name"),
            Some(&json!("bound-book"))
        );
        assert_eq!(
            export_value.pointer("/data/character_book/extensions"),
            Some(&json!({}))
        );
        assert_eq!(
            export_value.pointer("/data/character_book/entries/0/content"),
            Some(&json!("content"))
        );

        let updated = service
            .update_character(
                "Stale Export",
                UpdateCharacterDto {
                    name: None,
                    chat: None,
                    description: None,
                    personality: None,
                    scenario: None,
                    first_mes: None,
                    mes_example: None,
                    creator: None,
                    creator_notes: None,
                    character_version: None,
                    tags: None,
                    talkativeness: None,
                    fav: None,
                    alternate_greetings: None,
                    system_prompt: None,
                    post_history_instructions: None,
                    extensions: Some(json!({ "world": "" })),
                },
            )
            .await
            .expect("unbind world");

        assert_eq!(
            updated.extensions,
            Some(json!({
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            }))
        );

        character_repository
            .clear_cache()
            .await
            .expect("clear stale repository cache");
        let stored = character_repository
            .find_by_name("Stale Export")
            .await
            .expect("load updated character");
        assert!(stored.data.character_book.is_none());
        assert_eq!(stored.data.extensions.world, "");

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn export_character_uses_current_world_entries_without_mutating_source_card() {
        let (service, character_repository, world_info_repository, root) = setup_service().await;
        let _original_book =
            save_world_with_stale_original_data(&world_info_repository, "bound-book").await;

        let mut character = Character::new(
            "Export File".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hello".to_string(),
        );
        character.data.extensions.world = "bound-book".to_string();
        character_repository
            .save(&character)
            .await
            .expect("save stale character");

        let export_path = root.join("exported.json");
        service
            .export_character(ExportCharacterDto {
                name: "Export File".to_string(),
                target_path: export_path.to_string_lossy().into_owned(),
            })
            .await
            .expect("export character");

        let exported_json = fs::read_to_string(&export_path)
            .await
            .expect("read exported json");
        let exported_value: serde_json::Value =
            serde_json::from_str(&exported_json).expect("parse exported json");
        assert_eq!(
            exported_value.pointer("/data/character_book/name"),
            Some(&json!("bound-book"))
        );
        assert_eq!(
            exported_value.pointer("/data/character_book/extensions"),
            Some(&json!({}))
        );
        assert_eq!(
            exported_value.pointer("/data/character_book/description"),
            Some(&json!("preserve me"))
        );
        assert_eq!(
            exported_value.pointer("/data/character_book/entries/0/id"),
            Some(&json!(7))
        );
        assert_eq!(
            exported_value.pointer("/data/character_book/entries/0/content"),
            Some(&json!("fresh"))
        );
        assert_eq!(
            exported_value.pointer("/data/character_book/entries/0/extensions/custom"),
            Some(&json!("value"))
        );

        character_repository
            .clear_cache()
            .await
            .expect("clear stale repository cache");
        let stored = character_repository
            .find_by_name("Export File")
            .await
            .expect("reload source character");
        assert!(stored.data.character_book.is_none());

        let _ = fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn update_avatar_materializes_bound_lorebook_into_written_card() {
        let (service, character_repository, world_info_repository, root) = setup_service().await;
        save_bound_world(&world_info_repository, "bound-book").await;

        let mut character = Character::new(
            "Avatar Export".to_string(),
            "desc".to_string(),
            "persona".to_string(),
            "hello".to_string(),
        );
        character.data.extensions.world = "bound-book".to_string();
        character_repository
            .save(&character)
            .await
            .expect("save stale character");

        let avatar_path = root.join("replacement.png");
        fs::write(&avatar_path, build_minimal_png())
            .await
            .expect("write replacement avatar");

        service
            .update_avatar(UpdateAvatarDto {
                name: "Avatar Export".to_string(),
                avatar_path: avatar_path.to_string_lossy().into_owned(),
                crop: None,
            })
            .await
            .expect("update avatar");

        character_repository
            .clear_cache()
            .await
            .expect("clear stale repository cache");
        let stored = character_repository
            .find_by_name("Avatar Export")
            .await
            .expect("reload updated character");
        assert_eq!(
            stored
                .data
                .character_book
                .as_ref()
                .and_then(|value| value.get("name")),
            Some(&json!("bound-book"))
        );
        assert_eq!(
            stored
                .data
                .character_book
                .as_ref()
                .and_then(|value| value.pointer("/entries/0/content")),
            Some(&json!("content"))
        );

        let _ = fs::remove_dir_all(&root).await;
    }
}
