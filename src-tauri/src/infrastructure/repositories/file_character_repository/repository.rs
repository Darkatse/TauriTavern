use std::path::Path;

use async_trait::async_trait;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::domain::errors::DomainError;
use crate::domain::models::character::Character;
use crate::domain::models::chat::parse_message_timestamp_value;
use crate::domain::repositories::character_repository::{
    CharacterChat, CharacterRepository, ImageCrop,
};
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::png_utils::{
    process_avatar_image, write_character_data_to_png,
};

use super::FileCharacterRepository;

impl FileCharacterRepository {
    fn with_storage_identity(character: &Character, file_name: &str) -> Character {
        let mut stored = character.clone();
        stored.file_name = Some(file_name.to_string());
        stored.avatar = format!("{}.png", file_name);
        stored.shallow = false;
        stored
    }
}

#[async_trait]
impl CharacterRepository for FileCharacterRepository {
    async fn save(&self, character: &Character) -> Result<(), DomainError> {
        self.ensure_directory_exists().await?;

        let file_name = character.get_file_name();
        let file_path = self.get_character_path(&file_name);

        let image_data = if file_path.exists() {
            fs::read(&file_path).await.map_err(|e| {
                logger::error(&format!("Failed to read character file: {}", e));
                DomainError::InternalError(format!("Failed to read character file: {}", e))
            })?
        } else {
            self.read_default_avatar().await?
        };

        let character_v2 = character.to_v2();

        let json_data = serde_json::to_string(&character_v2).map_err(|e| {
            logger::error(&format!("Failed to serialize character: {}", e));
            DomainError::InvalidData(format!("Failed to serialize character: {}", e))
        })?;

        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        fs::write(&file_path, new_image_data).await.map_err(|e| {
            logger::error(&format!("Failed to write character file: {}", e));
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        let cached_character = Self::with_storage_identity(character, &file_name);

        let mut cache = self.memory_cache.lock().await;
        cache.set(file_name, cached_character);

        Ok(())
    }

    async fn find_by_name(&self, name: &str) -> Result<Character, DomainError> {
        let cached = {
            let cache = self.memory_cache.lock().await;
            cache.get(name)
        };

        if let Some(character) = cached {
            if !character.shallow {
                return Ok(character);
            }
        }

        let file_path = self.get_character_path(name);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                name
            )));
        }

        let character = self.read_character_from_file(&file_path).await?;

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
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                name
            )));
        }

        fs::remove_file(&file_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete character file: {}", e));
            DomainError::InternalError(format!("Failed to delete character file: {}", e))
        })?;

        if delete_chats {
            let chat_dir = self.get_chat_directory(name);
            if chat_dir.exists() {
                fs::remove_dir_all(&chat_dir).await.map_err(|e| {
                    logger::error(&format!("Failed to delete chat directory: {}", e));
                    DomainError::InternalError(format!("Failed to delete chat directory: {}", e))
                })?;
            }
        }

        let mut cache = self.memory_cache.lock().await;
        cache.remove(name);

        Ok(())
    }

    async fn update(&self, character: &Character) -> Result<(), DomainError> {
        let file_name = character.get_file_name();
        let file_path = self.get_character_path(&file_name);

        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                file_name
            )));
        }

        self.save(character).await
    }

    async fn rename(&self, old_name: &str, new_name: &str) -> Result<Character, DomainError> {
        let old_path = self.get_character_path(old_name);
        if !old_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                old_name
            )));
        }

        let new_path = self.get_character_path(new_name);
        if new_path.exists() {
            return Err(DomainError::InvalidData(format!(
                "Character already exists: {}",
                new_name
            )));
        }

        let mut character = self.read_character_from_file(&old_path).await?;

        character.name = new_name.to_string();
        character.data.name = new_name.to_string();
        let character = Self::with_storage_identity(&character, new_name);

        self.save(&character).await?;

        let old_chat_dir = self.get_chat_directory(old_name);
        let new_chat_dir = self.get_chat_directory(new_name);

        if old_chat_dir.exists() && !new_chat_dir.exists() {
            fs::rename(&old_chat_dir, &new_chat_dir)
                .await
                .map_err(|e| {
                    logger::error(&format!("Failed to rename chat directory: {}", e));
                    DomainError::InternalError(format!("Failed to rename chat directory: {}", e))
                })?;
        }

        fs::remove_file(&old_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete old character file: {}", e));
            DomainError::InternalError(format!("Failed to delete old character file: {}", e))
        })?;

        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(old_name);
        }

        Ok(character)
    }

    async fn import_character(
        &self,
        file_path: &Path,
        preserve_file_name: Option<String>,
    ) -> Result<Character, DomainError> {
        self.ensure_directory_exists().await?;

        let file_data = fs::read(file_path).await.map_err(|e| {
            logger::error(&format!("Failed to read file: {}", e));
            DomainError::InternalError(format!("Failed to read file: {}", e))
        })?;

        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "png" => {
                self.import_from_png_file(file_path, &file_data, preserve_file_name.as_deref())
                    .await
            }
            "json" => {
                self.import_from_json_file(file_path, file_data, preserve_file_name.as_deref())
                    .await
            }
            _ => Err(DomainError::InvalidData(format!(
                "Unsupported file format: {}",
                extension
            ))),
        }
    }

    async fn export_character(&self, name: &str, target_path: &Path) -> Result<(), DomainError> {
        self.find_by_name(name).await?;

        let file_path = self.get_character_path(name);

        fs::copy(&file_path, target_path).await.map_err(|e| {
            logger::error(&format!("Failed to export character: {}", e));
            DomainError::InternalError(format!("Failed to export character: {}", e))
        })?;

        Ok(())
    }

    async fn export_character_png_bytes(
        &self,
        name: &str,
        character_card_json: &str,
    ) -> Result<Vec<u8>, DomainError> {
        let file_path = self.get_character_path(name);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                name
            )));
        }

        let image_data = fs::read(&file_path).await.map_err(|e| {
            logger::error(&format!(
                "Failed to read character file for export {}: {}",
                file_path.display(),
                e
            ));
            DomainError::InternalError(format!("Failed to read character file: {}", e))
        })?;

        write_character_data_to_png(&image_data, character_card_json)
    }

    async fn create_with_avatar(
        &self,
        character: &Character,
        avatar_path: Option<&Path>,
        crop: Option<ImageCrop>,
    ) -> Result<Character, DomainError> {
        let image_data = if let Some(path) = avatar_path {
            let file_data = fs::read(path).await.map_err(|e| {
                logger::error(&format!("Failed to read avatar file: {}", e));
                DomainError::InternalError(format!("Failed to read avatar file: {}", e))
            })?;

            process_avatar_image(&file_data, crop).await?
        } else {
            self.read_default_avatar().await?
        };

        let character_v2 = character.to_v2();

        let json_data = serde_json::to_string(&character_v2).map_err(|e| {
            logger::error(&format!("Failed to serialize character: {}", e));
            DomainError::InvalidData(format!("Failed to serialize character: {}", e))
        })?;

        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        let file_name = character.get_file_name();
        let file_path = self.get_character_path(&file_name);

        fs::write(&file_path, new_image_data).await.map_err(|e| {
            logger::error(&format!("Failed to write character file: {}", e));
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        let stored_character = Self::with_storage_identity(character, &file_name);

        let mut cache = self.memory_cache.lock().await;
        cache.set(file_name, stored_character.clone());

        Ok(stored_character)
    }

    async fn update_avatar(
        &self,
        name: &str,
        avatar_path: &Path,
        crop: Option<ImageCrop>,
    ) -> Result<(), DomainError> {
        let character = self.find_by_name(name).await?;

        let file_data = fs::read(avatar_path).await.map_err(|e| {
            logger::error(&format!("Failed to read avatar file: {}", e));
            DomainError::InternalError(format!("Failed to read avatar file: {}", e))
        })?;

        let image_data = process_avatar_image(&file_data, crop).await?;

        let character_v2 = character.to_v2();

        let json_data = serde_json::to_string(&character_v2).map_err(|e| {
            logger::error(&format!("Failed to serialize character: {}", e));
            DomainError::InvalidData(format!("Failed to serialize character: {}", e))
        })?;

        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        let file_path = self.get_character_path(name);

        fs::write(&file_path, new_image_data).await.map_err(|e| {
            logger::error(&format!("Failed to write character file: {}", e));
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        Ok(())
    }

    async fn get_character_chats(
        &self,
        name: &str,
        simple: bool,
    ) -> Result<Vec<CharacterChat>, DomainError> {
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

            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

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

            let metadata = fs::metadata(&path).await.map_err(|e| {
                tracing::error!("Failed to read file metadata: {}", e);
                DomainError::InternalError(format!("Failed to read file metadata: {}", e))
            })?;

            let file_size = format!("{:.2}kb", metadata.len() as f64 / 1024.0);
            let fallback_date = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or(0);

            let file = fs::File::open(&path).await.map_err(|e| {
                tracing::error!("Failed to open chat file: {}", e);
                DomainError::InternalError(format!("Failed to open chat file: {}", e))
            })?;
            let reader = BufReader::new(file);
            let mut lines = reader.lines();
            let mut line_count = 0usize;
            let mut last_non_empty_line: Option<String> = None;

            while let Some(line) = lines.next_line().await.map_err(|e| {
                tracing::error!("Failed to read line from chat file: {}", e);
                DomainError::InternalError(format!("Failed to read line from chat file: {}", e))
            })? {
                if line.trim().is_empty() {
                    continue;
                }
                line_count = line_count.saturating_add(1);
                last_non_empty_line = Some(line);
            }

            let chat_items = line_count.saturating_sub(1);

            let (last_message, last_message_date) = if let Some(last_line) = last_non_empty_line {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&last_line) {
                    let message = json
                        .get("mes")
                        .and_then(|m| m.as_str())
                        .unwrap_or("[The chat is empty]")
                        .to_string();
                    let date = parse_message_timestamp_value(json.get("send_date"));
                    let date = if date > 0 { date } else { fallback_date };
                    (message, date)
                } else {
                    ("[Invalid chat format]".to_string(), fallback_date)
                }
            } else {
                ("[The chat is empty]".to_string(), fallback_date)
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
