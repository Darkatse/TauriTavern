use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Instant, Duration};
use async_trait::async_trait;
use tokio::fs;
use tokio::sync::Mutex;
use tokio::io::AsyncReadExt;

use crate::domain::errors::DomainError;
use crate::domain::models::character::{Character, sanitize_filename};
use crate::domain::repositories::character_repository::{CharacterRepository, ImageCrop, CharacterChat};
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file, list_files_with_extension, delete_file};
use crate::infrastructure::persistence::png_utils::{read_character_data_from_png, write_character_data_to_png, process_avatar_image, parse_character_from_png, write_character_to_png};

/// Memory cache for character data
struct MemoryCache {
    characters: HashMap<String, (Character, Instant)>,
    capacity: usize,
    ttl: Duration,
}

impl MemoryCache {
    /// Create a new memory cache with the specified capacity and TTL
    fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            characters: HashMap::with_capacity(capacity),
            capacity,
            ttl,
        }
    }

    /// Get a character from the cache
    fn get(&self, name: &str) -> Option<Character> {
        if let Some((character, timestamp)) = self.characters.get(name) {
            if timestamp.elapsed() < self.ttl {
                return Some(character.clone());
            }
        }
        None
    }

    /// Set a character in the cache
    fn set(&mut self, name: String, character: Character) {
        // If we're at capacity, remove the oldest entry
        if self.characters.len() >= self.capacity && !self.characters.contains_key(&name) {
            if let Some((oldest_key, _)) = self.characters.iter()
                .min_by_key(|(_, (_, timestamp))| timestamp.elapsed()) {
                let oldest_key = oldest_key.clone();
                self.characters.remove(&oldest_key);
            }
        }

        self.characters.insert(name, (character, Instant::now()));
    }

    /// Remove a character from the cache
    fn remove(&mut self, name: &str) {
        self.characters.remove(name);
    }

    /// Clear the cache
    fn clear(&mut self) {
        self.characters.clear();
    }
}

/// File-based character repository implementation
pub struct FileCharacterRepository {
    characters_dir: PathBuf,
    chats_dir: PathBuf,
    default_avatar_path: PathBuf,
    memory_cache: Arc<Mutex<MemoryCache>>,
}

impl FileCharacterRepository {
    /// Create a new FileCharacterRepository
    pub fn new(characters_dir: PathBuf, chats_dir: PathBuf, default_avatar_path: PathBuf) -> Self {
        // Create a memory cache with 100 character capacity and 30 minute TTL
        let memory_cache = Arc::new(Mutex::new(
            MemoryCache::new(100, Duration::from_secs(30 * 60))
        ));

        Self {
            characters_dir,
            chats_dir,
            default_avatar_path,
            memory_cache,
        }
    }

    /// Ensure the characters directory exists
    async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.characters_dir.exists() {
            tracing::info!("Creating characters directory: {:?}", self.characters_dir);
            fs::create_dir_all(&self.characters_dir).await.map_err(|e| {
                tracing::error!("Failed to create characters directory: {}", e);
                DomainError::InternalError(format!("Failed to create characters directory: {}", e))
            })?;
        }

        if !self.chats_dir.exists() {
            tracing::info!("Creating chats directory: {:?}", self.chats_dir);
            fs::create_dir_all(&self.chats_dir).await.map_err(|e| {
                tracing::error!("Failed to create chats directory: {}", e);
                DomainError::InternalError(format!("Failed to create chats directory: {}", e))
            })?;
        }

        Ok(())
    }

    /// Get the path to a character file
    fn get_character_path(&self, name: &str) -> PathBuf {
        self.characters_dir.join(format!("{}.png", name))
    }

    /// Get the path to a character's chat directory
    fn get_chat_directory(&self, name: &str) -> PathBuf {
        self.chats_dir.join(name)
    }

    /// Calculate chat size and last chat date for a character
    async fn calculate_chat_stats(&self, name: &str) -> Result<(u64, i64), DomainError> {
        let chat_dir = self.get_chat_directory(name);

        if !chat_dir.exists() {
            return Ok((0, 0));
        }

        let mut entries = fs::read_dir(&chat_dir).await.map_err(|e| {
            tracing::error!("Failed to read chat directory: {}", e);
            DomainError::InternalError(format!("Failed to read chat directory: {}", e))
        })?;

        let mut total_size = 0;
        let mut latest_modified = 0;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            tracing::error!("Failed to read directory entry: {}", e);
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let metadata = entry.metadata().await.map_err(|e| {
                tracing::error!("Failed to read file metadata: {}", e);
                DomainError::InternalError(format!("Failed to read file metadata: {}", e))
            })?;

            if metadata.is_file() {
                total_size += metadata.len();

                if let Ok(modified) = metadata.modified() {
                    if let Ok(modified_time) = modified.duration_since(std::time::UNIX_EPOCH) {
                        let modified_ms = modified_time.as_millis() as i64;
                        if modified_ms > latest_modified {
                            latest_modified = modified_ms;
                        }
                    }
                }
            }
        }

        Ok((total_size, latest_modified))
    }

    /// Read a character from a PNG file
    async fn read_character_from_file(&self, path: &Path) -> Result<Character, DomainError> {
        logger::debug(&format!("Reading character from file: {:?}", path));

        // Read the file
        let file_data = fs::read(path).await.map_err(|e| {
            logger::error(&format!("Failed to read character file: {}", e));
            DomainError::InternalError(format!("Failed to read character file: {}", e))
        })?;

        // Extract character data
        let json_data = read_character_data_from_png(&file_data)?;

        // Parse character data
        let mut character: Character = serde_json::from_str(&json_data).map_err(|e| {
            logger::error(&format!("Failed to parse character data: {}", e));
            DomainError::InvalidData(format!("Failed to parse character data: {}", e))
        })?;

        // Set file name
        let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        character.file_name = Some(file_name.clone());

        // Set avatar
        character.avatar = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();

        // Set JSON data
        character.json_data = Some(json_data);

        // Get file metadata
        let metadata = fs::metadata(path).await.map_err(|e| {
            logger::error(&format!("Failed to read file metadata: {}", e));
            DomainError::InternalError(format!("Failed to read file metadata: {}", e))
        })?;

        // Set date added
        if let Ok(created) = metadata.created() {
            if let Ok(created_time) = created.duration_since(std::time::UNIX_EPOCH) {
                character.date_added = created_time.as_millis() as i64;
            }
        }

        // Calculate chat stats
        let (chat_size, date_last_chat) = self.calculate_chat_stats(&file_name).await?;
        character.chat_size = chat_size;
        character.date_last_chat = date_last_chat;

        Ok(character)
    }

    /// Process a character for the character list
    async fn process_character(&self, file_name: &str, shallow: bool) -> Result<Character, DomainError> {
        // Try to get from cache first
        {
            let cache = self.memory_cache.lock().await;
            if let Some(character) = cache.get(file_name) {
                // If shallow is requested, we can return directly from cache
                if shallow {
                    return Ok(character);
                }

                // For non-shallow requests, we need to update chat stats
                let mut character = character.clone();
                let (chat_size, date_last_chat) = self.calculate_chat_stats(file_name).await?;
                character.chat_size = chat_size;
                character.date_last_chat = date_last_chat;
                return Ok(character);
            }
        }

        // Not in cache, read from file
        let path = self.get_character_path(file_name);
        let character = self.read_character_from_file(&path).await?;

        // Update cache
        {
            let mut cache = self.memory_cache.lock().await;
            cache.set(file_name.to_string(), character.clone());
        }

        Ok(character)
    }

    /// Load all characters
    async fn load_all_characters(&self, shallow: bool) -> Result<Vec<Character>, DomainError> {
        self.ensure_directory_exists().await?;

        let character_files = list_files_with_extension(&self.characters_dir, "png").await?;
        let mut characters = Vec::new();

        for file_path in character_files {
            let file_name = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

            match self.process_character(&file_name, shallow).await {
                Ok(character) => {
                    characters.push(character);
                },
                Err(e) => {
                    logger::error(&format!("Failed to process character {}: {}", file_name, e));
                    // Continue processing other characters
                }
            }
        }

        Ok(characters)
    }
}

#[async_trait]
impl CharacterRepository for FileCharacterRepository {
    async fn save(&self, character: &Character) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        // Get the character file name
        let file_name = character.get_file_name();
        let file_path = self.get_character_path(&file_name);

        // Read the default avatar if the file doesn't exist
        let image_data = if file_path.exists() {
            fs::read(&file_path).await.map_err(|e| {
                logger::error(&format!("Failed to read character file: {}", e));
                DomainError::InternalError(format!("Failed to read character file: {}", e))
            })?
        } else {
            fs::read(&self.default_avatar_path).await.map_err(|e| {
                logger::error(&format!("Failed to read default avatar: {}", e));
                DomainError::InternalError(format!("Failed to read default avatar: {}", e))
            })?
        };

        // Convert character to V2 format
        let character_v2 = character.to_v2();

        // Write character data to PNG
        let json_data = serde_json::to_string(&character_v2).map_err(|e| {
            logger::error(&format!("Failed to serialize character: {}", e));
            DomainError::InvalidData(format!("Failed to serialize character: {}", e))
        })?;

        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        // Write the file
        fs::write(&file_path, new_image_data).await.map_err(|e| {
            logger::error(&format!("Failed to write character file: {}", e));
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        // Update cache
        let mut cache = self.memory_cache.lock().await;
        cache.set(file_name, character.clone());

        Ok(())
    }

    async fn find_by_name(&self, name: &str) -> Result<Character, DomainError> {
        // Try to get from cache first
        {
            let cache = self.memory_cache.lock().await;
            if let Some(character) = cache.get(name) {
                return Ok(character);
            }
        }

        // If not in cache, load from file
        let file_path = self.get_character_path(name);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!("Character not found: {}", name)));
        }

        let character = self.read_character_from_file(&file_path).await?;

        // Update cache
        let mut cache = self.memory_cache.lock().await;
        cache.set(name.to_string(), character.clone());

        Ok(character)
    }

    async fn find_all(&self, shallow: bool) -> Result<Vec<Character>, DomainError> {
        self.load_all_characters(shallow).await
    }

    async fn delete(&self, name: &str, delete_chats: bool) -> Result<(), DomainError> {
        let file_path = self.get_character_path(name);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!("Character not found: {}", name)));
        }

        // Delete character file
        fs::remove_file(&file_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete character file: {}", e));
            DomainError::InternalError(format!("Failed to delete character file: {}", e))
        })?;

        // Delete chats if requested
        if delete_chats {
            let chat_dir = self.get_chat_directory(name);
            if chat_dir.exists() {
                fs::remove_dir_all(&chat_dir).await.map_err(|e| {
                    logger::error(&format!("Failed to delete chat directory: {}", e));
                    DomainError::InternalError(format!("Failed to delete chat directory: {}", e))
                })?;
            }
        }

        // Update cache
        let mut cache = self.memory_cache.lock().await;
        cache.remove(name);

        Ok(())
    }

    async fn update(&self, character: &Character) -> Result<(), DomainError> {
        // Get the character file name
        let file_name = character.get_file_name();
        let file_path = self.get_character_path(&file_name);

        // Check if character exists
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!("Character not found: {}", file_name)));
        }

        // Save the updated character
        self.save(character).await
    }

    async fn rename(&self, old_name: &str, new_name: &str) -> Result<Character, DomainError> {
        // Check if old character exists
        let old_path = self.get_character_path(old_name);
        if !old_path.exists() {
            return Err(DomainError::NotFound(format!("Character not found: {}", old_name)));
        }

        // Check if new name already exists
        let new_path = self.get_character_path(new_name);
        if new_path.exists() {
            return Err(DomainError::InvalidData(format!("Character already exists: {}", new_name)));
        }

        // Read the character
        let mut character = self.read_character_from_file(&old_path).await?;

        // Update the name
        character.name = new_name.to_string();
        character.data.name = new_name.to_string();
        character.file_name = Some(new_name.to_string());

        // Save the character with the new name
        self.save(&character).await?;

        // Rename chat directory if it exists
        let old_chat_dir = self.get_chat_directory(old_name);
        let new_chat_dir = self.get_chat_directory(new_name);

        if old_chat_dir.exists() && !new_chat_dir.exists() {
            fs::rename(&old_chat_dir, &new_chat_dir).await.map_err(|e| {
                logger::error(&format!("Failed to rename chat directory: {}", e));
                DomainError::InternalError(format!("Failed to rename chat directory: {}", e))
            })?;
        }

        // Delete the old character file
        fs::remove_file(&old_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete old character file: {}", e));
            DomainError::InternalError(format!("Failed to delete old character file: {}", e))
        })?;

        // Update cache
        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(old_name);
            cache.set(new_name.to_string(), character.clone());
        }

        Ok(character)
    }

    async fn import_character(&self, file_path: &Path, preserve_file_name: Option<String>) -> Result<Character, DomainError> {
        // Read the file
        let file_data = fs::read(file_path).await.map_err(|e| {
            logger::error(&format!("Failed to read file: {}", e));
            DomainError::InternalError(format!("Failed to read file: {}", e))
        })?;

        // Get the file extension
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

        let mut character = match extension.as_str() {
            "png" => {
                // Parse character from PNG
                let json_data = read_character_data_from_png(&file_data)?;
                let mut character: Character = serde_json::from_str(&json_data).map_err(|e| {
                    logger::error(&format!("Failed to parse character data: {}", e));
                    DomainError::InvalidData(format!("Failed to parse character data: {}", e))
                })?;

                // Set avatar
                character.avatar = "none".to_string();

                character
            },
            "json" => {
                // Parse character from JSON
                let json_data = String::from_utf8(file_data).map_err(|e| {
                    logger::error(&format!("Failed to parse JSON file: {}", e));
                    DomainError::InvalidData(format!("Failed to parse JSON file: {}", e))
                })?;

                let mut character: Character = serde_json::from_str(&json_data).map_err(|e| {
                    logger::error(&format!("Failed to parse character data: {}", e));
                    DomainError::InvalidData(format!("Failed to parse character data: {}", e))
                })?;

                // Set avatar
                character.avatar = "none".to_string();

                character
            },
            _ => {
                return Err(DomainError::InvalidData(format!("Unsupported file format: {}", extension)));
            }
        };

        // Set file name
        if let Some(name) = preserve_file_name {
            character.file_name = Some(name);
        }

        // Save the character
        self.save(&character).await?;

        Ok(character)
    }

    async fn export_character(&self, name: &str, target_path: &Path) -> Result<(), DomainError> {
        // Find the character
        let character = self.find_by_name(name).await?;

        // Get the character file
        let file_path = self.get_character_path(name);

        // Copy the file
        fs::copy(&file_path, target_path).await.map_err(|e| {
            logger::error(&format!("Failed to export character: {}", e));
            DomainError::InternalError(format!("Failed to export character: {}", e))
        })?;

        Ok(())
    }

    async fn create_with_avatar(&self, character: &Character, avatar_path: Option<&Path>, crop: Option<ImageCrop>) -> Result<Character, DomainError> {
        // Process avatar image if provided
        let image_data = if let Some(path) = avatar_path {
            // Read the avatar file
            let file_data = fs::read(path).await.map_err(|e| {
                logger::error(&format!("Failed to read avatar file: {}", e));
                DomainError::InternalError(format!("Failed to read avatar file: {}", e))
            })?;

            // Process the image
            process_avatar_image(&file_data, crop).await?
        } else {
            // Use default avatar
            fs::read(&self.default_avatar_path).await.map_err(|e| {
                logger::error(&format!("Failed to read default avatar: {}", e));
                DomainError::InternalError(format!("Failed to read default avatar: {}", e))
            })?
        };

        // Convert character to V2 format
        let character_v2 = character.to_v2();

        // Write character data to PNG
        let json_data = serde_json::to_string(&character_v2).map_err(|e| {
            logger::error(&format!("Failed to serialize character: {}", e));
            DomainError::InvalidData(format!("Failed to serialize character: {}", e))
        })?;

        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        // Get the character file name
        let file_name = character.get_file_name();
        let file_path = self.get_character_path(&file_name);

        // Write the file
        fs::write(&file_path, new_image_data).await.map_err(|e| {
            logger::error(&format!("Failed to write character file: {}", e));
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        // Update cache
        let mut cache = self.memory_cache.lock().await;
        cache.set(file_name.clone(), character.clone());

        // Return the character
        Ok(character.clone())
    }

    async fn update_avatar(&self, name: &str, avatar_path: &Path, crop: Option<ImageCrop>) -> Result<(), DomainError> {
        // Find the character
        let character = self.find_by_name(name).await?;

        // Read the avatar file
        let file_data = fs::read(avatar_path).await.map_err(|e| {
            logger::error(&format!("Failed to read avatar file: {}", e));
            DomainError::InternalError(format!("Failed to read avatar file: {}", e))
        })?;

        // Process the image
        let image_data = process_avatar_image(&file_data, crop).await?;

        // Convert character to V2 format
        let character_v2 = character.to_v2();

        // Write character data to PNG
        let json_data = serde_json::to_string(&character_v2).map_err(|e| {
            logger::error(&format!("Failed to serialize character: {}", e));
            DomainError::InvalidData(format!("Failed to serialize character: {}", e))
        })?;

        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        // Get the character file path
        let file_path = self.get_character_path(name);

        // Write the file
        fs::write(&file_path, new_image_data).await.map_err(|e| {
            logger::error(&format!("Failed to write character file: {}", e));
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        Ok(())
    }

    async fn get_character_chats(&self, name: &str, simple: bool) -> Result<Vec<CharacterChat>, DomainError> {
        let chat_dir = self.get_chat_directory(name);

        if !chat_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&chat_dir).await.map_err(|e| {
            tracing::error!("Failed to read chat directory: {}", e);
            DomainError::InternalError(format!("Failed to read chat directory: {}", e))
        })?;

        let mut chats = Vec::new();

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            tracing::error!("Failed to read directory entry: {}", e);
            DomainError::InternalError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();

            // Only process JSONL files
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();

            if simple {
                chats.push(CharacterChat {
                    file_name,
                    file_size: "".to_string(),
                    chat_items: 0,
                    last_message: "".to_string(),
                    last_message_date: 0,
                });
                continue;
            }

            // Get file metadata
            let metadata = fs::metadata(&path).await.map_err(|e| {
                tracing::error!("Failed to read file metadata: {}", e);
                DomainError::InternalError(format!("Failed to read file metadata: {}", e))
            })?;

            let file_size = format!("{:.2}kb", metadata.len() as f64 / 1024.0);

            // Read the last line of the file to get the last message
            let file_content = fs::read_to_string(&path).await.map_err(|e| {
                tracing::error!("Failed to read chat file: {}", e);
                DomainError::InternalError(format!("Failed to read chat file: {}", e))
            })?;

            let lines: Vec<&str> = file_content.lines().collect();
            let chat_items = lines.len().saturating_sub(1); // First line is metadata

            let (last_message, last_message_date) = if let Some(last_line) = lines.last() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(last_line) {
                    let message = json.get("mes").and_then(|m| m.as_str()).unwrap_or("[The chat is empty]").to_string();
                    let date = json.get("send_date").and_then(|d| d.as_i64()).unwrap_or(0);
                    (message, date)
                } else {
                    ("[Invalid chat format]".to_string(), 0)
                }
            } else {
                ("[The chat is empty]".to_string(), 0)
            };

            chats.push(CharacterChat {
                file_name,
                file_size,
                chat_items,
                last_message,
                last_message_date,
            });
        }

        Ok(chats)
    }

    async fn clear_cache(&self) -> Result<(), DomainError> {
        let mut cache = self.memory_cache.lock().await;
        cache.clear();
        Ok(())
    }
}
