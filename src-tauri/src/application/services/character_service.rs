use crate::application::dto::character_dto::{
    CharacterChatDto, CharacterDto, CreateCharacterDto, CreateWithAvatarDto, DeleteCharacterDto,
    ExportCharacterDto, GetCharacterChatsDto, ImageCropDto, ImportCharacterDto, RenameCharacterDto,
    UpdateAvatarDto, UpdateCharacterDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::character::Character;
use crate::domain::repositories::character_repository::{
    CharacterChat, CharacterRepository, ImageCrop,
};
use crate::infrastructure::logging::logger;
use std::path::Path;
use std::sync::Arc;

/// Service for character management
pub struct CharacterService {
    repository: Arc<dyn CharacterRepository>,
}

impl CharacterService {
    /// Create a new CharacterService
    pub fn new(repository: Arc<dyn CharacterRepository>) -> Self {
        Self { repository }
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
        let character = self
            .repository
            .import_character(Path::new(&dto.file_path), dto.preserve_file_name)
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
}
