use std::sync::Arc;
use std::path::Path;
use chrono::Utc;

use crate::domain::models::chat::{Chat, ChatMessage, MessageExtra};
use crate::domain::repositories::chat_repository::{ChatRepository, ChatImportFormat, ChatExportFormat};
use crate::domain::repositories::character_repository::CharacterRepository;
use crate::application::dto::chat_dto::{
    ChatDto, ChatMessageDto, MessageExtraDto, ChatSearchResultDto,
    CreateChatDto, AddMessageDto, RenameChatDto, ImportChatDto, ExportChatDto
};
use crate::application::errors::ApplicationError;
use crate::infrastructure::logging::logger;

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
        tracing::info!("{}", &format!("Creating chat for character: {}", dto.character_name));

        // Verify that the character exists
        self.character_repository.find_by_name(&dto.character_name).await?;

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
    pub async fn get_chat(&self, character_name: &str, file_name: &str) -> Result<ChatDto, ApplicationError> {
        tracing::info!("Getting chat: {}/{}", character_name, file_name);

        let chat = self.chat_repository.get_chat(character_name, file_name).await?;

        Ok(ChatDto::from(chat))
    }

    /// Get all chats for a character
    pub async fn get_character_chats(&self, character_name: &str) -> Result<Vec<ChatDto>, ApplicationError> {
        tracing::info!("Getting chats for character: {}", character_name);

        // Verify that the character exists
        self.character_repository.find_by_name(character_name).await?;

        let chats = self.chat_repository.get_character_chats(character_name).await?;

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
        tracing::info!("Adding message to chat: {}/{}", dto.character_name, dto.file_name);

        // Create the message
        let message = if dto.is_user {
            // Get the chat to get the user name
            let chat = self.chat_repository.get_chat(&dto.character_name, &dto.file_name).await?;
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
        let chat = self.chat_repository.add_message(&dto.character_name, &dto.file_name, message).await?;

        Ok(ChatDto::from(chat))
    }

    /// Rename a chat
    pub async fn rename_chat(&self, dto: RenameChatDto) -> Result<(), ApplicationError> {
        tracing::info!("Renaming chat: {}/{} -> {}/{}",
            dto.character_name, dto.old_file_name, dto.character_name, dto.new_file_name);

        self.chat_repository.rename_chat(&dto.character_name, &dto.old_file_name, &dto.new_file_name).await?;

        Ok(())
    }

    /// Delete a chat
    pub async fn delete_chat(&self, character_name: &str, file_name: &str) -> Result<(), ApplicationError> {
        tracing::info!("Deleting chat: {}/{}", character_name, file_name);

        self.chat_repository.delete_chat(character_name, file_name).await?;

        Ok(())
    }

    /// Search for chats
    pub async fn search_chats(&self, query: &str, character_filter: Option<&str>) -> Result<Vec<ChatSearchResultDto>, ApplicationError> {
        tracing::info!("Searching chats for: {}", query);

        let results = self.chat_repository.search_chats(query, character_filter).await?;

        Ok(results.into_iter().map(ChatSearchResultDto::from).collect())
    }

    /// Import a chat
    pub async fn import_chat(&self, dto: ImportChatDto) -> Result<ChatDto, ApplicationError> {
        tracing::info!("Importing chat for character {} from {}", dto.character_name, dto.file_path);

        // Verify that the character exists
        self.character_repository.find_by_name(&dto.character_name).await?;

        // Convert the format string to enum
        let format = ChatImportFormat::from(dto.format);

        // Import the chat
        let chat = self.chat_repository.import_chat(
            &dto.character_name,
            Path::new(&dto.file_path),
            format
        ).await?;

        Ok(ChatDto::from(chat))
    }

    /// Export a chat
    pub async fn export_chat(&self, dto: ExportChatDto) -> Result<(), ApplicationError> {
        tracing::info!("Exporting chat: {}/{} to {}", dto.character_name, dto.file_name, dto.target_path);

        // Convert the format string to enum
        let format = ChatExportFormat::from(dto.format);

        // Export the chat
        self.chat_repository.export_chat(
            &dto.character_name,
            &dto.file_name,
            Path::new(&dto.target_path),
            format
        ).await?;

        Ok(())
    }

    /// Backup a chat
    pub async fn backup_chat(&self, character_name: &str, file_name: &str) -> Result<(), ApplicationError> {
        tracing::info!("Backing up chat: {}/{}", character_name, file_name);

        self.chat_repository.backup_chat(character_name, file_name).await?;

        Ok(())
    }

    /// Clear the chat cache
    pub async fn clear_cache(&self) -> Result<(), ApplicationError> {
        tracing::info!("Clearing chat cache");

        self.chat_repository.clear_cache().await?;

        Ok(())
    }
}
