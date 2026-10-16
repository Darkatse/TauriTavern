mod card_contract;
mod lorebook_adoption;
mod lorebook_codec;

use crate::dto::character_dto::{
    BulkMergeCharacterCardDataDto, BulkMergeCharacterCardDataResultDto, CharacterChatDto,
    CharacterDto, CharacterLorebookAdoptionDto, CreateCharacterDto,
    CreateCharacterWithAvatarResultDto, CreateWithAvatarDto, DeleteCharacterDto,
    DuplicateCharacterDto, ExportCharacterContentDto, ExportCharacterContentResultDto,
    GetCharacterChatsDto, ImportCharacterDto, ImportCharacterResultDto, MergeCharacterCardDataDto,
    RenameCharacterDto, ReplaceCharacterDto, UpdateAvatarDto, UpdateCharacterCardDataDto,
    UpdateCharacterDto,
};
use crate::errors::ApplicationError;
use crate::services::agent_workspace_lifecycle_service::{
    AgentChatWorkspaceTarget, AgentWorkspaceLifecycleService,
};
use crate::services::chat_history_coordinator::ChatHistoryCoordinator;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tt_contracts::client_asset_paths::validate_path_segment;
use tt_domain::errors::DomainError;
use tt_domain::json_merge::{merge_json_value, merge_json_value_with_unset};
use tt_domain::models::character::Character;
use tt_ports::repositories::character_repository::{CharacterRepository, ImageCrop};
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::world_info_repository::WorldInfoRepository;

use self::lorebook_codec::world_info_to_character_book;

/// Service for character management
pub struct CharacterService {
    repository: Arc<dyn CharacterRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    world_info_repository: Arc<dyn WorldInfoRepository>,
    agent_workspace_lifecycle_service: Arc<AgentWorkspaceLifecycleService>,
    chat_history_coordinator: Arc<ChatHistoryCoordinator>,
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
        chat_repository: Arc<dyn ChatRepository>,
        world_info_repository: Arc<dyn WorldInfoRepository>,
        agent_workspace_lifecycle_service: Arc<AgentWorkspaceLifecycleService>,
        chat_history_coordinator: Arc<ChatHistoryCoordinator>,
    ) -> Self {
        Self {
            repository,
            chat_repository,
            world_info_repository,
            agent_workspace_lifecycle_service,
            chat_history_coordinator,
        }
    }

    /// Get all characters
    pub async fn get_all_characters(
        &self,
        shallow: bool,
    ) -> Result<Vec<CharacterDto>, ApplicationError> {
        tracing::debug!("Getting all characters");
        let characters = self.repository.find_all(shallow).await?;
        Ok(characters.into_iter().map(CharacterDto::from).collect())
    }

    /// Get a character by name
    pub async fn get_character(&self, name: &str) -> Result<CharacterDto, ApplicationError> {
        tracing::debug!("Getting character: {}", name);
        let character = self.repository.find_by_name(name).await?;
        Ok(CharacterDto::from(character))
    }

    /// Create a new character
    pub async fn create_character(
        &self,
        dto: CreateCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        tracing::debug!("Creating character: {}", dto.name);
        let primary_lorebook = dto.primary_lorebook.clone();

        // Convert DTO to domain model
        let mut character = Character::try_from(dto).map_err(Self::map_extensions_error)?;

        // Validate character
        self.validate_character(&character)?;
        self.materialize_create_lorebook(&mut character, primary_lorebook.as_deref())
            .await;

        let created = self
            .repository
            .create_with_avatar(&character, None, None)
            .await?;

        Ok(CharacterDto::from(created.character))
    }

    /// Create a character with an avatar
    pub async fn create_with_avatar(
        &self,
        dto: CreateWithAvatarDto,
    ) -> Result<CreateCharacterWithAvatarResultDto, ApplicationError> {
        tracing::debug!("Creating character with avatar: {}", dto.character.name);
        let primary_lorebook = dto.character.primary_lorebook.clone();

        // Convert DTO to domain model
        let mut character =
            Character::try_from(dto.character).map_err(Self::map_extensions_error)?;

        // Validate character
        self.validate_character(&character)?;
        self.materialize_create_lorebook(&mut character, primary_lorebook.as_deref())
            .await;

        // Convert avatar path
        let avatar_path_ref: Option<&Path> = dto.avatar_path.as_deref().map(Path::new);

        // Convert crop parameters
        let crop = dto.crop.map(ImageCrop::from);

        // Create character with avatar
        let created = self
            .repository
            .create_with_avatar(&character, avatar_path_ref, crop)
            .await?;

        Ok(CreateCharacterWithAvatarResultDto::from(created))
    }

    /// Update a character
    pub async fn update_character(
        &self,
        name: &str,
        dto: UpdateCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        tracing::debug!("Updating character: {}", name);
        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut card_value = card_contract::parse_character_card_json(&raw_json)?;
        let UpdateCharacterDto {
            name: character_name,
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

        let mut root_update = serde_json::Map::new();
        let mut data_update = serde_json::Map::new();
        let mut extension_update = match extensions {
            Some(Value::Object(extensions)) => extensions,
            Some(_) => {
                return Err(ApplicationError::ValidationError(
                    "Invalid character extensions: expected a JSON object".to_string(),
                ));
            }
            None => serde_json::Map::new(),
        };

        for (field, value) in [
            ("name", character_name.map(Value::String)),
            ("description", description.map(Value::String)),
            ("personality", personality.map(Value::String)),
            ("scenario", scenario.map(Value::String)),
            ("first_mes", first_mes.map(Value::String)),
            ("mes_example", mes_example.map(Value::String)),
            ("creator", creator.map(Value::String)),
            ("creator_notes", creator_notes.map(Value::String)),
            ("character_version", character_version.map(Value::String)),
            ("tags", tags.map(|tags| serde_json::json!(tags))),
        ] {
            if let Some(value) = value {
                root_update.insert(field.to_string(), value.clone());
                data_update.insert(field.to_string(), value);
            }
        }

        if let Some(chat) = chat {
            root_update.insert("chat".to_string(), Value::String(chat));
        }
        if let Some(alternate_greetings) = alternate_greetings {
            data_update.insert(
                "alternate_greetings".to_string(),
                serde_json::json!(alternate_greetings),
            );
        }
        if let Some(system_prompt) = system_prompt {
            data_update.insert("system_prompt".to_string(), Value::String(system_prompt));
        }
        if let Some(post_history_instructions) = post_history_instructions {
            data_update.insert(
                "post_history_instructions".to_string(),
                Value::String(post_history_instructions),
            );
        }
        if let Some(talkativeness) = talkativeness {
            let value = serde_json::json!(talkativeness);
            root_update.insert("talkativeness".to_string(), value.clone());
            extension_update.insert("talkativeness".to_string(), value);
        }
        if let Some(fav) = fav {
            let value = Value::Bool(fav);
            root_update.insert("fav".to_string(), value.clone());
            extension_update.insert("fav".to_string(), value);
        }
        if !extension_update.is_empty() {
            data_update.insert("extensions".to_string(), Value::Object(extension_update));
        }
        if !data_update.is_empty() {
            root_update.insert("data".to_string(), Value::Object(data_update));
        }
        merge_json_value(&mut card_value, Value::Object(root_update));

        let updated = self
            .write_character_card_value(
                name,
                card_value,
                None,
                None,
                CharacterCardValidationMode::ReadableOnly,
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
        tracing::debug!("Updating character card data: {}", name);

        let UpdateCharacterCardDataDto {
            card_json,
            avatar_path,
            crop,
            materialize_primary_lorebook,
        } = dto;
        let crop = crop.map(ImageCrop::from);
        let avatar_path = avatar_path.as_deref().map(Path::new);
        let mut card_value = card_contract::parse_character_card_json(&card_json)?;
        if materialize_primary_lorebook
            && let Err(error) = self
                .materialize_primary_lorebook_value(&mut card_value)
                .await
        {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Saving character '{}' without refreshing its linked lorebook: {}",
                name,
                error
            );
        }
        let updated = self
            .write_character_card_value(
                name,
                card_value,
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
        tracing::debug!("Merging character card data: {}", name);

        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut card_value = card_contract::parse_character_card_json(&raw_json)?;
        merge_json_value_with_unset(&mut card_value, dto.update);
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

    /// Merge raw attributes into many stored character cards using upstream-compatible bulk semantics.
    pub async fn bulk_merge_character_card_data(
        &self,
        dto: BulkMergeCharacterCardDataDto,
    ) -> Result<BulkMergeCharacterCardDataResultDto, ApplicationError> {
        if !dto.data.is_object() {
            return Err(ApplicationError::ValidationError(
                "No valid update data provided.".to_string(),
            ));
        }

        let target_avatars = if dto.avatars.is_empty() {
            self.repository.list_avatar_filenames().await?
        } else {
            dto.avatars
        };

        let filter_path = dto
            .filter
            .as_ref()
            .map(|filter| filter.path.trim())
            .filter(|path| !path.is_empty());
        let mut result = BulkMergeCharacterCardDataResultDto {
            updated: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
        };

        for avatar in target_avatars {
            let avatar = Self::normalize_merge_avatar_filename(&avatar)?;
            let name = Self::avatar_file_stem(&avatar);
            let merge_result = self
                .merge_character_card_value_for_bulk(name, dto.data.clone(), filter_path)
                .await;

            match merge_result {
                Ok(true) => result.updated.push(avatar),
                Ok(false) => result.skipped.push(avatar),
                Err(error) => {
                    tracing::warn!("Bulk character merge failed for {}: {}", avatar, error);
                    result.failed.push(avatar);
                }
            }
        }

        Ok(result)
    }

    /// Delete a character
    pub async fn delete_character(&self, dto: DeleteCharacterDto) -> Result<(), ApplicationError> {
        tracing::debug!("Deleting character: {}", dto.name);
        let _run_guard = self
            .agent_workspace_lifecycle_service
            .lock_run_lifecycle()
            .await;
        let linked_world = match self.repository.read_character_card_json(&dto.name).await {
            Ok(raw_json) => match serde_json::from_str::<Value>(&raw_json) {
                Ok(card) => card
                    .pointer("/data/extensions/world")
                    .and_then(Value::as_str)
                    .filter(|world| !world.is_empty())
                    .map(ToString::to_string),
                Err(error) => {
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Deleting character '{}' without resolving its linked lorebook: {}",
                        dto.name,
                        error
                    );
                    None
                }
            },
            Err(error) => {
                tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Deleting character '{}' without resolving its linked lorebook: {}",
                    dto.name,
                    error
                );
                None
            }
        };
        let workspace_targets = if dto.delete_chats {
            self.agent_workspace_targets_for_character_chats(&dto.name)
                .await?
        } else {
            Vec::new()
        };
        self.agent_workspace_lifecycle_service
            .ensure_chat_workspaces_inactive(&workspace_targets)
            .await?;

        let execution_guard = self
            .chat_history_coordinator
            .lock_snapshot_execution()
            .await;
        self.chat_history_coordinator
            .invalidate_character(&dto.name)
            .await;
        self.repository.delete(&dto.name, dto.delete_chats).await?;
        self.chat_history_coordinator
            .invalidate_character(&dto.name)
            .await;
        drop(execution_guard);
        if let Err(error) = self
            .agent_workspace_lifecycle_service
            .delete_chat_workspaces_locked(&workspace_targets)
            .await
        {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Deleted character '{}' but could not delete all Agent workspaces: {}",
                dto.name,
                error
            );
        }
        if let Some(world) = linked_world
            && let Err(error) = self.world_info_repository.delete_world_info(&world).await
            && !matches!(&error, DomainError::NotFound(_))
        {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Deleted character '{}' but could not delete linked lorebook '{}': {}",
                dto.name,
                world,
                error
            );
        }
        Ok(())
    }

    async fn agent_workspace_targets_for_character_chats(
        &self,
        character_name: &str,
    ) -> Result<Vec<AgentChatWorkspaceTarget>, ApplicationError> {
        let identities = self
            .chat_repository
            .list_character_chat_identities(character_name)
            .await?;
        let mut targets = Vec::new();
        for identity in identities {
            if let Some(integrity) = identity.integrity {
                targets.push(AgentWorkspaceLifecycleService::character_target(
                    character_name,
                    &identity.file_name,
                    &integrity,
                )?);
            }
        }
        Ok(targets)
    }

    /// Rename a character
    pub async fn rename_character(
        &self,
        dto: RenameCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        self.validate_character_name(&dto.new_name)?;

        tracing::debug!("Renaming character: {} -> {}", dto.old_name, dto.new_name);
        let execution_guard = self
            .chat_history_coordinator
            .lock_snapshot_execution()
            .await;
        self.chat_history_coordinator
            .invalidate_character(&dto.old_name)
            .await;
        self.chat_history_coordinator
            .invalidate_character(&dto.new_name)
            .await;
        let character = self.repository.rename(&dto.old_name, &dto.new_name).await?;
        self.chat_history_coordinator
            .invalidate_character(&dto.old_name)
            .await;
        self.chat_history_coordinator
            .invalidate_character(&dto.new_name)
            .await;
        drop(execution_guard);
        Ok(CharacterDto::from(character))
    }

    /// Duplicate a character using upstream file-copy semantics.
    pub async fn duplicate_character(
        &self,
        dto: DuplicateCharacterDto,
    ) -> Result<CharacterDto, ApplicationError> {
        tracing::debug!("Duplicating character: {}", dto.name);
        let character = self.repository.duplicate(&dto.name).await?;
        Ok(CharacterDto::from(character))
    }

    /// Import a character and adopt its embedded lorebook without replacing existing worlds.
    pub async fn import_character(
        &self,
        dto: ImportCharacterDto,
    ) -> Result<ImportCharacterResultDto, ApplicationError> {
        tracing::debug!("Importing character from: {}", dto.file_path);
        let character = self
            .repository
            .import_character(Path::new(&dto.file_path), dto.preserve_file_name)
            .await?;
        Ok(self.finish_character_import(character, "").await)
    }

    /// Replace a stored character and adopt its embedded lorebook.
    pub async fn replace_character(
        &self,
        dto: ReplaceCharacterDto,
    ) -> Result<ImportCharacterResultDto, ApplicationError> {
        tracing::debug!("Replacing character {} from: {}", dto.name, dto.file_path);
        if !validate_path_segment(&dto.name) {
            return Err(ApplicationError::ValidationError(
                "Character storage identity is invalid".to_string(),
            ));
        }
        let primary_lorebook = match self
            .repository
            .read_character_card_json(&dto.name)
            .await
            .map_err(ApplicationError::from)
            .and_then(|raw| card_contract::parse_character_card_json(&raw))
        {
            Ok(card) => card
                .pointer("/data/extensions/world")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            Err(error) => {
                tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Replacing character '{}' without preserving its optional lorebook binding: {}",
                    dto.name,
                    error
                );
                String::new()
            }
        };
        let character = self
            .repository
            .replace_character(Path::new(&dto.file_path), &dto.name, &primary_lorebook)
            .await?;

        Ok(self
            .finish_character_import(character, &primary_lorebook)
            .await)
    }

    async fn finish_character_import(
        &self,
        mut character: Character,
        previous: &str,
    ) -> ImportCharacterResultDto {
        let lorebook = match self.adopt_embedded_lorebook(&mut character, previous).await {
            Ok(lorebook) => lorebook,
            Err(error) => {
                tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Character '{}' was imported, but its embedded lorebook could not be adopted: {}",
                    character.name,
                    error
                );
                CharacterLorebookAdoptionDto::Failed
            }
        };
        ImportCharacterResultDto {
            character: CharacterDto::from(character),
            lorebook,
        }
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

        Ok(ExportCharacterContentResultDto { data: png_bytes })
    }

    /// Update a character's avatar
    pub async fn update_avatar(&self, dto: UpdateAvatarDto) -> Result<(), ApplicationError> {
        tracing::debug!("Updating avatar for character: {}", dto.name);
        let raw_json = self.repository.read_character_card_json(&dto.name).await?;
        let crop = dto.crop.map(ImageCrop::from);
        self.write_character_card_value(
            &dto.name,
            card_contract::parse_character_card_json(&raw_json)?,
            Some(Path::new(&dto.avatar_path)),
            crop,
            CharacterCardValidationMode::ReadableOnly,
        )
        .await?;
        Ok(())
    }

    /// Get character chats
    pub async fn get_character_chats(
        &self,
        dto: GetCharacterChatsDto,
    ) -> Result<Vec<CharacterChatDto>, ApplicationError> {
        tracing::debug!("Getting chats for character: {}", dto.name);
        let chats = self
            .repository
            .get_character_chats(&dto.name, dto.simple)
            .await?;
        Ok(chats.into_iter().map(CharacterChatDto::from).collect())
    }

    /// Clear the character cache
    pub async fn clear_cache(&self) -> Result<(), DomainError> {
        tracing::debug!("Clearing character cache");
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

    fn normalize_merge_avatar_filename(avatar: &str) -> Result<String, ApplicationError> {
        let value = avatar;
        let is_png = value
            .get(value.len().saturating_sub(4)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".png"));

        if value.is_empty()
            || value.contains('/')
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || !is_png
        {
            return Err(ApplicationError::ValidationError(format!(
                "Invalid avatar filename: {}",
                avatar
            )));
        }

        Ok(value.to_string())
    }

    fn avatar_file_stem(avatar: &str) -> &str {
        if avatar
            .get(avatar.len().saturating_sub(4)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".png"))
        {
            &avatar[..avatar.len() - 4]
        } else {
            avatar
        }
    }

    fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
        if path.trim().is_empty() {
            return Some(value);
        }

        let mut current = value;
        for segment in path.split('.') {
            if segment.is_empty() {
                return None;
            }

            current = match current {
                Value::Object(object) => object.get(segment)?,
                Value::Array(array) => {
                    let index = segment.parse::<usize>().ok()?;
                    array.get(index)?
                }
                _ => return None,
            };
        }

        Some(current)
    }

    async fn merge_character_card_value_for_bulk(
        &self,
        name: &str,
        update: Value,
        filter_path: Option<&str>,
    ) -> Result<bool, ApplicationError> {
        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut card_value = card_contract::parse_character_card_json(&raw_json)?;

        if let Some(filter_path) = filter_path
            && Self::value_at_path(&card_value, filter_path).is_none()
        {
            return Ok(false);
        }

        merge_json_value_with_unset(&mut card_value, update);
        self.write_character_card_value(
            name,
            card_value,
            None,
            None,
            CharacterCardValidationMode::ReadableOnly,
        )
        .await?;

        Ok(true)
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
        let card_json =
            self.prepare_character_card_json_for_write(&mut card_value, validation_mode)?;

        self.repository
            .write_character_card_json(name, &card_json, avatar_path, crop)
            .await
            .map_err(Into::into)
    }

    fn prepare_character_card_json_for_write(
        &self,
        card_value: &mut Value,
        validation_mode: CharacterCardValidationMode,
    ) -> Result<String, ApplicationError> {
        card_contract::strip_character_card_json_data(card_value);
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
                self.validate_character_name(name).map_err(Into::into)
            }
            CharacterCardValidationMode::Strict => self
                .validate_character_card_value(card_value)
                .map_err(Into::into),
        }
    }

    fn validate_character_card_value(&self, card_value: &Value) -> Result<(), DomainError> {
        card_contract::validate_character_card_schema(card_value)?;
        let name = card_contract::character_card_name(card_value)?;
        self.validate_character_name(name)
    }

    async fn materialize_create_lorebook(
        &self,
        character: &mut Character,
        primary_lorebook: Option<&str>,
    ) {
        let Some(world_name) = primary_lorebook.filter(|value| !value.is_empty()) else {
            return;
        };

        let materialized = match self.world_info_repository.get_world_info(world_name).await {
            Ok(Some(world_info)) => {
                Self::apply_materialized_lorebook(character, world_name, &world_info)
            }
            Ok(None) => Err(DomainError::NotFound(format!(
                "World info file {} doesn't exist",
                world_name
            ))),
            Err(error) => Err(error),
        };

        if let Err(error) = materialized {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Creating character '{}' without embedding lorebook '{}': {}",
                character.name,
                world_name,
                error
            );
        }
    }

    fn apply_materialized_lorebook(
        character: &mut Character,
        world_name: &str,
        world_info: &Value,
    ) -> Result<(), DomainError> {
        let character_book = world_info_to_character_book(world_name, world_info)?;
        character.data.character_book = Some(character_book);
        Ok(())
    }

    async fn build_export_card_value(&self, name: &str) -> Result<Value, DomainError> {
        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut export_value: Value = serde_json::from_str(&raw_json).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to parse stored character payload: {}",
                error
            ))
        })?;

        if let Err(error) = self
            .materialize_primary_lorebook_value(&mut export_value)
            .await
        {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Exporting character '{}' with its stored embedded lorebook because the linked lorebook could not be refreshed: {}",
                name,
                error
            );
        }
        card_contract::unset_private_fields(&mut export_value)?;
        card_contract::sanitize_agent_profiles_for_export(&mut export_value);

        Ok(export_value)
    }

    async fn materialize_primary_lorebook_value(
        &self,
        export_value: &mut Value,
    ) -> Result<(), DomainError> {
        let Some(world_name) = export_value
            .pointer("/data/extensions/world")
            .and_then(Value::as_str)
        else {
            return Ok(());
        };
        if world_name.is_empty() {
            return Ok(());
        }

        let world_info = self
            .world_info_repository
            .get_world_info(world_name)
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
