use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::character::Character;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::list_files_with_extension;
use crate::infrastructure::persistence::png_utils::read_character_data_from_png;

use super::FileCharacterRepository;

impl FileCharacterRepository {
    pub(crate) async fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.characters_dir.exists() {
            tracing::info!("Creating characters directory: {:?}", self.characters_dir);
            fs::create_dir_all(&self.characters_dir)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create characters directory: {}", e);
                    DomainError::InternalError(format!(
                        "Failed to create characters directory: {}",
                        e
                    ))
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

    pub(crate) fn get_character_path(&self, name: &str) -> PathBuf {
        self.characters_dir.join(format!("{}.png", name))
    }

    pub(crate) fn get_chat_directory(&self, name: &str) -> PathBuf {
        self.chats_dir.join(name)
    }

    pub(crate) async fn calculate_chat_stats(&self, name: &str) -> Result<(u64, i64), DomainError> {
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

    pub(crate) async fn read_character_from_file(
        &self,
        path: &Path,
    ) -> Result<Character, DomainError> {
        logger::debug(&format!("Reading character from file: {:?}", path));

        let file_data = fs::read(path).await.map_err(|e| {
            logger::error(&format!("Failed to read character file: {}", e));
            DomainError::InternalError(format!("Failed to read character file: {}", e))
        })?;

        let json_data = read_character_data_from_png(&file_data)?;

        let mut character: Character = serde_json::from_str(&json_data).map_err(|e| {
            logger::error(&format!("Failed to parse character data: {}", e));
            DomainError::InvalidData(format!("Failed to parse character data: {}", e))
        })?;
        self.normalize_imported_character(&mut character)?;
        character.shallow = false;

        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        character.file_name = Some(file_name.clone());

        character.avatar = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        character.json_data = None;

        let metadata = fs::metadata(path).await.map_err(|e| {
            logger::error(&format!("Failed to read file metadata: {}", e));
            DomainError::InternalError(format!("Failed to read file metadata: {}", e))
        })?;

        if let Ok(created) = metadata.created() {
            if let Ok(created_time) = created.duration_since(std::time::UNIX_EPOCH) {
                character.date_added = created_time.as_millis() as i64;
            }
        }

        let (chat_size, date_last_chat) = self.calculate_chat_stats(&file_name).await?;
        character.chat_size = chat_size;
        character.date_last_chat = date_last_chat;

        Ok(character)
    }

    pub(crate) async fn process_character(
        &self,
        file_name: &str,
        shallow: bool,
    ) -> Result<Character, DomainError> {
        let cached = {
            let cache = self.memory_cache.lock().await;
            cache.get(file_name)
        };

        if let Some(character) = cached {
            if shallow {
                if character.shallow {
                    return Ok(character);
                }
                return Ok(character.to_shallow());
            }

            if !character.shallow {
                let mut character = character;
                let (chat_size, date_last_chat) = self.calculate_chat_stats(file_name).await?;
                character.chat_size = chat_size;
                character.date_last_chat = date_last_chat;
                return Ok(character);
            }
        }

        let path = self.get_character_path(file_name);
        let character = self.read_character_from_file(&path).await?;
        let result = if shallow {
            character.to_shallow()
        } else {
            character
        };

        {
            let mut cache = self.memory_cache.lock().await;
            cache.set(file_name.to_string(), result.clone());
        }

        Ok(result)
    }

    pub(crate) async fn load_all_characters(
        &self,
        shallow: bool,
    ) -> Result<Vec<Character>, DomainError> {
        self.ensure_directory_exists().await?;

        let character_files = list_files_with_extension(&self.characters_dir, "png").await?;
        let mut characters = Vec::new();

        for file_path in character_files {
            let file_name = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            match self.process_character(&file_name, shallow).await {
                Ok(character) => {
                    characters.push(character);
                }
                Err(e) => {
                    logger::error(&format!("Failed to process character {}: {}", file_name, e));
                }
            }
        }

        Ok(characters)
    }

    pub(crate) async fn read_default_avatar(&self) -> Result<Vec<u8>, DomainError> {
        match fs::read(&self.default_avatar_path).await {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                logger::warn(&format!(
                    "Default avatar not found at {:?}, using generated placeholder image",
                    self.default_avatar_path
                ));
                Self::generate_placeholder_avatar_png()
            }
            Err(error) => {
                logger::error(&format!("Failed to read default avatar: {}", error));
                Err(DomainError::InternalError(format!(
                    "Failed to read default avatar: {}",
                    error
                )))
            }
        }
    }

    pub(crate) fn generate_placeholder_avatar_png() -> Result<Vec<u8>, DomainError> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])));
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);

        image.write_to(&mut cursor, ImageFormat::Png).map_err(|e| {
            DomainError::InternalError(format!("Failed to create fallback avatar: {}", e))
        })?;

        Ok(output)
    }
}
