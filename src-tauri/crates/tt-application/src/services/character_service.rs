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
};
use crate::errors::ApplicationError;
use crate::services::agent_workspace_lifecycle_service::{
    AgentChatWorkspaceTarget, AgentWorkspaceLifecycleService,
};
use crate::services::chat_history_coordinator::ChatHistoryCoordinator;
use chrono::{SecondsFormat, Utc};
use serde::de::IgnoredAny;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tt_contracts::client_asset_paths::validate_path_segment;
use tt_domain::errors::DomainError;
use tt_domain::json_document::JsonDocument;
use tt_domain::models::character::Character;
use tt_domain::models::chat::humanized_date;
use tt_ports::avatar_images::ImageCrop;
use tt_ports::repositories::character_repository::{CharacterCreateResult, CharacterRepository};
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
        let created = self.create_character_card(dto, None, None).await?;
        Ok(CharacterDto::from(created.character))
    }

    /// Create a character with an avatar
    pub async fn create_with_avatar(
        &self,
        dto: CreateWithAvatarDto,
    ) -> Result<CreateCharacterWithAvatarResultDto, ApplicationError> {
        let avatar_path = dto.avatar_path.as_deref().map(Path::new);
        let crop = dto.crop.map(ImageCrop::from);
        let created = self
            .create_character_card(dto.character, avatar_path, crop)
            .await?;
        Ok(CreateCharacterWithAvatarResultDto::from(created))
    }

    async fn create_character_card(
        &self,
        dto: CreateCharacterDto,
        avatar_path: Option<&Path>,
        crop: Option<ImageCrop>,
    ) -> Result<CharacterCreateResult, ApplicationError> {
        let mut card = card_contract::parse_character_card_json(&dto.card_json)?;
        let name = card
            .get::<String>(&["name"])
            .map_err(card_contract::invalid_character_card_json)?
            .unwrap_or_default();
        self.validate_character_name(&name)?;
        tracing::debug!("Creating character: {}", name);

        if let Some(world_name) = dto
            .primary_lorebook
            .as_deref()
            .filter(|world_name| !world_name.is_empty())
            && let Err(error) = self.materialize_lorebook(&mut card, world_name).await
        {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Creating character '{}' without embedding lorebook '{}': {}",
                name,
                world_name,
                error
            );
        }

        let now = Utc::now();
        let chat = format!("{} - {}", name, humanized_date(now));
        card.set("chat", &Value::from(chat));
        card.set(
            "create_date",
            &Value::from(now.to_rfc3339_opts(SecondsFormat::Millis, true)),
        );

        let file_name = dto
            .file_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.strip_suffix(".png").unwrap_or(value))
            .unwrap_or(&name);
        let card_json = card_contract::into_character_card_json(card);

        self.repository
            .create_with_avatar(file_name, &card_json, avatar_path, crop)
            .await
            .map_err(Into::into)
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
        let mut card = card_contract::parse_character_card_json(&card_json)?;
        if materialize_primary_lorebook
            && let Err(error) = self.materialize_primary_lorebook(&mut card).await
        {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Saving character '{}' without refreshing its linked lorebook: {}",
                name,
                error
            );
        }
        let updated = self
            .write_character_card(name, card, avatar_path, crop)
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
        let mut card = card_contract::parse_character_card_json(&raw_json)?;
        let update = card_contract::parse_character_card_json(&dto.update_json)?;
        card.merge_with_unset(update)
            .map_err(card_contract::invalid_character_card_json)?;
        let card_json = card_contract::into_character_card_json(card);
        card_contract::validate_character_card_schema(&card_json)?;
        let updated = self
            .repository
            .write_character_card_json(name, &card_json, None, None)
            .await?;

        Ok(CharacterDto::from(updated))
    }

    /// Merge raw attributes into many stored character cards using upstream-compatible bulk semantics.
    pub async fn bulk_merge_character_card_data(
        &self,
        dto: BulkMergeCharacterCardDataDto,
    ) -> Result<BulkMergeCharacterCardDataResultDto, ApplicationError> {
        let update = JsonDocument::parse(dto.data_json.as_bytes()).map_err(|_| {
            ApplicationError::ValidationError("No valid update data provided.".to_string())
        })?;

        let target_avatars = if dto.avatars.is_empty() {
            self.repository.list_avatar_filenames().await?
        } else {
            dto.avatars
        };

        let filter_path: Option<Vec<&str>> = dto
            .filter
            .as_ref()
            .map(|filter| filter.path.trim())
            .filter(|path| !path.is_empty())
            .map(|path| path.split('.').collect());
        let mut result = BulkMergeCharacterCardDataResultDto {
            updated: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
        };

        for avatar in target_avatars {
            let avatar = Self::normalize_merge_avatar_filename(&avatar)?;
            let name = Self::avatar_file_stem(&avatar);
            let merge_result = self
                .merge_character_card_for_bulk(name, update.clone(), filter_path.as_deref())
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
            .and_then(|raw| card_contract::parse_character_card_json(&raw))
            .and_then(|card| card_contract::character_world(&card))
        {
            Ok(world) => world.unwrap_or_default(),
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

        let card = self.build_export_card(&dto.name).await?;

        if format == "json" {
            return Ok(ExportCharacterContentResultDto {
                data: card.to_pretty_vec(),
            });
        }

        let png_bytes = self
            .repository
            .export_character_png_bytes(&dto.name, &card.to_string())
            .await?;

        Ok(ExportCharacterContentResultDto { data: png_bytes })
    }

    /// Update a character's avatar
    pub async fn update_avatar(&self, dto: UpdateAvatarDto) -> Result<(), ApplicationError> {
        tracing::debug!("Updating avatar for character: {}", dto.name);
        let card_json = self.repository.read_character_card_json(&dto.name).await?;
        let crop = dto.crop.map(ImageCrop::from);
        self.repository
            .write_character_card_json(
                &dto.name,
                &card_json,
                Some(Path::new(&dto.avatar_path)),
                crop,
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

    /// Upstream skips a card when `_.get(card, filter.path)` is undefined.
    async fn merge_character_card_for_bulk(
        &self,
        name: &str,
        update: JsonDocument,
        filter_path: Option<&[&str]>,
    ) -> Result<bool, ApplicationError> {
        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut card = card_contract::parse_character_card_json(&raw_json)?;

        if let Some(path) = filter_path
            && card
                .get::<IgnoredAny>(path)
                .map_err(card_contract::invalid_character_card_json)?
                .is_none()
        {
            return Ok(false);
        }

        card.merge_with_unset(update)
            .map_err(card_contract::invalid_character_card_json)?;
        self.write_character_card(name, card, None, None).await?;

        Ok(true)
    }

    async fn write_character_card(
        &self,
        name: &str,
        card: JsonDocument,
        avatar_path: Option<&Path>,
        crop: Option<ImageCrop>,
    ) -> Result<Character, ApplicationError> {
        let card_json = card_contract::into_character_card_json(card);

        self.repository
            .write_character_card_json(name, &card_json, avatar_path, crop)
            .await
            .map_err(Into::into)
    }

    async fn build_export_card(&self, name: &str) -> Result<JsonDocument, DomainError> {
        let raw_json = self.repository.read_character_card_json(name).await?;
        let mut card = card_contract::parse_character_card_json(&raw_json)?;

        if let Err(error) = self.materialize_primary_lorebook(&mut card).await {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Exporting character '{}' with its stored embedded lorebook because the linked lorebook could not be refreshed: {}",
                name,
                error
            );
        }
        card_contract::unset_private_fields(&mut card)?;
        card_contract::sanitize_agent_profiles_for_export(&mut card)?;

        Ok(card)
    }

    async fn materialize_primary_lorebook(
        &self,
        card: &mut JsonDocument,
    ) -> Result<(), DomainError> {
        match card_contract::character_world(card)? {
            Some(world_name) => self.materialize_lorebook(card, &world_name).await,
            None => Ok(()),
        }
    }

    async fn materialize_lorebook(
        &self,
        card: &mut JsonDocument,
        world_name: &str,
    ) -> Result<(), DomainError> {
        let world_info = self
            .world_info_repository
            .get_world_info(world_name)
            .await?
            .ok_or_else(|| {
                DomainError::NotFound(format!("World info file {} doesn't exist", world_name))
            })?;
        let character_book = world_info_to_character_book(world_name, &world_info)?;
        card.object_entry(&["data"])
            .map_err(card_contract::invalid_character_card_json)?
            .set("character_book", &character_book);

        Ok(())
    }
}

#[cfg(test)]
mod tests;
