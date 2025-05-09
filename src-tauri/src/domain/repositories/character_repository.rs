use async_trait::async_trait;
use std::path::Path;
use crate::domain::models::character::Character;
use crate::domain::errors::DomainError;

/// Repository interface for character management
#[async_trait]
pub trait CharacterRepository: Send + Sync {
    /// Save a character to the repository
    async fn save(&self, character: &Character) -> Result<(), DomainError>;

    /// Find a character by its file name (without extension)
    async fn find_by_name(&self, name: &str) -> Result<Character, DomainError>;

    /// Find all characters in the repository
    async fn find_all(&self, shallow: bool) -> Result<Vec<Character>, DomainError>;

    /// Delete a character by its file name (without extension)
    async fn delete(&self, name: &str, delete_chats: bool) -> Result<(), DomainError>;

    /// Update an existing character
    async fn update(&self, character: &Character) -> Result<(), DomainError>;

    /// Rename a character
    async fn rename(&self, old_name: &str, new_name: &str) -> Result<Character, DomainError>;

    /// Import a character from a file
    async fn import_character(&self, file_path: &Path, preserve_file_name: Option<String>) -> Result<Character, DomainError>;

    /// Export a character to a file
    async fn export_character(&self, name: &str, target_path: &Path) -> Result<(), DomainError>;

    /// Create a character with an avatar image
    async fn create_with_avatar(&self, character: &Character, avatar_path: Option<&Path>, crop: Option<ImageCrop>) -> Result<Character, DomainError>;

    /// Update a character's avatar
    async fn update_avatar(&self, name: &str, avatar_path: &Path, crop: Option<ImageCrop>) -> Result<(), DomainError>;

    /// Get character chats
    async fn get_character_chats(&self, name: &str, simple: bool) -> Result<Vec<CharacterChat>, DomainError>;

    /// Clear the character cache
    async fn clear_cache(&self) -> Result<(), DomainError>;
}

/// Image crop parameters
#[derive(Debug, Clone)]
pub struct ImageCrop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub want_resize: bool,
}

/// Character chat information
#[derive(Debug, Clone)]
pub struct CharacterChat {
    pub file_name: String,
    pub file_size: String,
    pub chat_items: usize,
    pub last_message: String,
    pub last_message_date: i64,
}
