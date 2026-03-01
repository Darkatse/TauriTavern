use std::path::Path;
use std::sync::Arc;

use crate::application::dto::chat_dto::{
    AddMessageDto, ChatDto, ChatSearchResultDto, CreateChatDto, DeleteGroupChatDto, ExportChatDto,
    ImportCharacterChatsDto, ImportChatDto, ImportGroupChatDto, RenameChatDto, RenameGroupChatDto,
    SaveChatFromFileDto, SaveGroupChatFromFileDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::chat::{Chat, ChatMessage, MessageExtra};
use crate::domain::repositories::character_repository::CharacterRepository;
use crate::domain::repositories::chat_repository::{
    ChatExportFormat, ChatImportFormat, ChatRepository, PinnedCharacterChat, PinnedGroupChat,
};

/// Service for managing chats
pub struct ChatService {
    chat_repository: Arc<dyn ChatRepository>,
    character_repository: Arc<dyn CharacterRepository>,
}

impl ChatService {
    /// Create a new ChatService
    pub fn new(
        chat_repository: Arc<dyn ChatRepository>,
        character_repository: Arc<dyn CharacterRepository>,
    ) -> Self {
        Self {
            chat_repository,
            character_repository,
        }
    }

    /// Create a new chat
    pub async fn create_chat(&self, dto: CreateChatDto) -> Result<ChatDto, ApplicationError> {
        tracing::info!("Creating chat for character: {}", dto.character_name);

        // Verify that the character exists
        self.character_repository
            .find_by_name(&dto.character_name)
            .await?;

        // Create a new chat
        let mut chat = Chat::new(&dto.user_name, &dto.character_name);

        // Add the first message if provided
        if let Some(first_message) = dto.first_message {
            let message = ChatMessage::character(&dto.character_name, &first_message);
            chat.add_message(message);
        }

        // Save the chat
        self.chat_repository.save(&chat).await?;

        Ok(ChatDto::from(chat))
    }

    /// Get a chat by character name and file name
    pub async fn get_chat(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<ChatDto, ApplicationError> {
        tracing::info!("Getting chat: {}/{}", character_name, file_name);

        let chat = self
            .chat_repository
            .get_chat(character_name, file_name)
            .await?;

        Ok(ChatDto::from(chat))
    }

    /// Get all chats for a character
    pub async fn get_character_chats(
        &self,
        character_name: &str,
    ) -> Result<Vec<ChatDto>, ApplicationError> {
        tracing::info!("Getting chats for character: {}", character_name);

        // Verify that the character exists
        self.character_repository
            .find_by_name(character_name)
            .await?;

        let chats = self
            .chat_repository
            .get_character_chats(character_name)
            .await?;

        Ok(chats.into_iter().map(ChatDto::from).collect())
    }

    /// Get all chats
    pub async fn get_all_chats(&self) -> Result<Vec<ChatDto>, ApplicationError> {
        tracing::info!("Getting all chats");

        let chats = self.chat_repository.get_all_chats().await?;

        Ok(chats.into_iter().map(ChatDto::from).collect())
    }

    /// Add a message to a chat
    pub async fn add_message(&self, dto: AddMessageDto) -> Result<ChatDto, ApplicationError> {
        tracing::info!(
            "Adding message to chat: {}/{}",
            dto.character_name,
            dto.file_name
        );

        // Create the message
        let message = if dto.is_user {
            // Get the chat to get the user name
            let chat = self
                .chat_repository
                .get_chat(&dto.character_name, &dto.file_name)
                .await?;
            ChatMessage::user(&chat.user_name, &dto.content)
        } else {
            ChatMessage::character(&dto.character_name, &dto.content)
        };

        // Add extra data if provided
        let message = if let Some(extra) = dto.extra {
            ChatMessage {
                extra: MessageExtra::from(extra),
                ..message
            }
        } else {
            message
        };

        // Add the message to the chat
        let chat = self
            .chat_repository
            .add_message(&dto.character_name, &dto.file_name, message)
            .await?;

        Ok(ChatDto::from(chat))
    }

    /// Rename a chat
    pub async fn rename_chat(&self, dto: RenameChatDto) -> Result<(), ApplicationError> {
        tracing::info!(
            "Renaming chat: {}/{} -> {}/{}",
            dto.character_name,
            dto.old_file_name,
            dto.character_name,
            dto.new_file_name
        );

        self.chat_repository
            .rename_chat(&dto.character_name, &dto.old_file_name, &dto.new_file_name)
            .await?;

        Ok(())
    }

    /// Delete a chat
    pub async fn delete_chat(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<(), ApplicationError> {
        tracing::info!("Deleting chat: {}/{}", character_name, file_name);

        self.chat_repository
            .delete_chat(character_name, file_name)
            .await?;

        Ok(())
    }

    /// Search for chats
    pub async fn search_chats(
        &self,
        query: &str,
        character_filter: Option<&str>,
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Searching chats for: {}", query);

        let results = self
            .chat_repository
            .search_chats(query, character_filter)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// List chat summaries without loading full chat payloads.
    pub async fn list_chat_summaries(
        &self,
        character_filter: Option<&str>,
        include_metadata: bool,
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Listing chat summaries");

        let results = self
            .chat_repository
            .list_chat_summaries(character_filter, include_metadata)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// List group chat summaries without loading full chat payloads.
    pub async fn list_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Listing group chat summaries");

        let results = self
            .chat_repository
            .list_group_chat_summaries(chat_ids, include_metadata)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// List recent character chat summaries without full summary scan.
    pub async fn list_recent_chat_summaries(
        &self,
        character_filter: Option<&str>,
        include_metadata: bool,
        max_entries: usize,
        pinned: &[PinnedCharacterChat],
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Listing recent character chat summaries");

        let results = self
            .chat_repository
            .list_recent_chat_summaries(character_filter, include_metadata, max_entries, pinned)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// List recent group chat summaries without full summary scan.
    pub async fn list_recent_group_chat_summaries(
        &self,
        chat_ids: Option<&[String]>,
        include_metadata: bool,
        max_entries: usize,
        pinned: &[PinnedGroupChat],
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Listing recent group chat summaries");

        let results = self
            .chat_repository
            .list_recent_group_chat_summaries(chat_ids, include_metadata, max_entries, pinned)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// Search group chats with optional chat ID filtering.
    pub async fn search_group_chats(
        &self,
        query: &str,
        chat_ids: Option<&[String]>,
    ) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Searching group chats");

        let results = self
            .chat_repository
            .search_group_chats(query, chat_ids)
            .await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// Import a chat
    pub async fn import_chat(&self, dto: ImportChatDto) -> Result<ChatDto, ApplicationError> {
        tracing::info!(
            "Importing chat for character {} from {}",
            dto.character_name,
            dto.file_path
        );

        // Verify that the character exists
        self.character_repository
            .find_by_name(&dto.character_name)
            .await?;

        // Convert the format string to enum
        let format = ChatImportFormat::from(dto.format);

        // Import the chat
        let chat = self
            .chat_repository
            .import_chat(&dto.character_name, Path::new(&dto.file_path), format)
            .await?;

        Ok(ChatDto::from(chat))
    }

    /// Export a chat
    pub async fn export_chat(&self, dto: ExportChatDto) -> Result<(), ApplicationError> {
        tracing::info!(
            "Exporting chat: {}/{} to {}",
            dto.character_name,
            dto.file_name,
            dto.target_path
        );

        // Convert the format string to enum
        let format = ChatExportFormat::from(dto.format);

        // Export the chat
        self.chat_repository
            .export_chat(
                &dto.character_name,
                &dto.file_name,
                Path::new(&dto.target_path),
                format,
            )
            .await?;

        Ok(())
    }

    /// Backup a chat
    pub async fn backup_chat(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<(), ApplicationError> {
        tracing::info!("Backing up chat: {}/{}", character_name, file_name);

        self.chat_repository
            .backup_chat(character_name, file_name)
            .await?;

        Ok(())
    }

    /// List chat backups.
    pub async fn list_chat_backups(&self) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Listing chat backups");

        let results = self.chat_repository.list_chat_backups().await?;
        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// Get raw bytes of a chat backup file.
    pub async fn get_chat_backup_bytes(
        &self,
        backup_file_name: &str,
    ) -> Result<Vec<u8>, ApplicationError> {
        if backup_file_name.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Backup file name cannot be empty".to_string(),
            ));
        }

        self.chat_repository
            .get_chat_backup_bytes(backup_file_name)
            .await
            .map_err(Into::into)
    }

    /// Delete a chat backup file.
    pub async fn delete_chat_backup(&self, backup_file_name: &str) -> Result<(), ApplicationError> {
        if backup_file_name.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Backup file name cannot be empty".to_string(),
            ));
        }

        self.chat_repository
            .delete_chat_backup(backup_file_name)
            .await
            .map_err(Into::into)
    }

    /// Clear the chat cache
    pub async fn clear_cache(&self) -> Result<(), ApplicationError> {
        tracing::info!("Clearing chat cache");

        self.chat_repository.clear_cache().await?;

        Ok(())
    }

    /// Get the absolute path to a character chat payload file.
    pub async fn get_chat_payload_path(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<String, ApplicationError> {
        let path = self
            .chat_repository
            .get_chat_payload_path(character_name, file_name)
            .await?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Save a character chat payload from a JSONL file path.
    pub async fn save_chat_from_file(
        &self,
        dto: SaveChatFromFileDto,
    ) -> Result<(), ApplicationError> {
        if dto.character_name.trim().is_empty() || dto.file_name.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Character name and file name cannot be empty".to_string(),
            ));
        }

        self.chat_repository
            .save_chat_payload_from_path(
                &dto.character_name,
                &dto.file_name,
                Path::new(&dto.file_path),
                dto.force.unwrap_or(false),
            )
            .await
            .map_err(Into::into)
    }

    /// Get the absolute path to a group chat payload file.
    pub async fn get_group_chat_payload_path(
        &self,
        chat_id: &str,
    ) -> Result<String, ApplicationError> {
        if chat_id.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Group chat id cannot be empty".to_string(),
            ));
        }

        let path = self
            .chat_repository
            .get_group_chat_payload_path(chat_id)
            .await?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Save a group chat payload from a JSONL file path.
    pub async fn save_group_chat_from_file(
        &self,
        dto: SaveGroupChatFromFileDto,
    ) -> Result<(), ApplicationError> {
        if dto.id.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Group chat id cannot be empty".to_string(),
            ));
        }

        self.chat_repository
            .save_group_chat_payload_from_path(
                &dto.id,
                Path::new(&dto.file_path),
                dto.force.unwrap_or(false),
            )
            .await
            .map_err(Into::into)
    }

    /// Delete a group chat payload file.
    pub async fn delete_group_chat(&self, dto: DeleteGroupChatDto) -> Result<(), ApplicationError> {
        if dto.id.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Group chat id cannot be empty".to_string(),
            ));
        }

        self.chat_repository
            .delete_group_chat_payload(&dto.id)
            .await?;
        Ok(())
    }

    /// Rename a group chat payload file.
    pub async fn rename_group_chat(&self, dto: RenameGroupChatDto) -> Result<(), ApplicationError> {
        if dto.old_file_name.trim().is_empty() || dto.new_file_name.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Group chat file name cannot be empty".to_string(),
            ));
        }

        self.chat_repository
            .rename_group_chat_payload(&dto.old_file_name, &dto.new_file_name)
            .await?;
        Ok(())
    }

    /// Import one or more character chats from an uploaded file.
    pub async fn import_character_chats(
        &self,
        dto: ImportCharacterChatsDto,
    ) -> Result<Vec<String>, ApplicationError> {
        if dto.character_name.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "Character name cannot be empty".to_string(),
            ));
        }

        let character_display_name = dto
            .character_display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&dto.character_name);
        let user_name = dto
            .user_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("User");

        self.chat_repository
            .import_chat_payload(
                &dto.character_name,
                character_display_name,
                user_name,
                Path::new(&dto.file_path),
                &dto.file_type,
            )
            .await
            .map_err(Into::into)
    }

    /// Import a group chat payload and return the created chat id.
    pub async fn import_group_chat(
        &self,
        dto: ImportGroupChatDto,
    ) -> Result<String, ApplicationError> {
        self.chat_repository
            .import_group_chat_payload(Path::new(&dto.file_path))
            .await
            .map_err(Into::into)
    }
}
