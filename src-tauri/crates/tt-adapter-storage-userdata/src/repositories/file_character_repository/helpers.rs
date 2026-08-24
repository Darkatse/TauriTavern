use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::Value;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tokio::fs;

use crate::png_card_metadata::read_character_data_from_png_file;
use tt_adapter_storage_core::chat_directory_identity::{self, SharedChatAliasStore};
use tt_adapter_storage_core::file_system::list_files_with_extension;
use tt_domain::errors::DomainError;
use tt_domain::models::character::Character;
use tt_domain::models::filename::sanitize_filename;

use super::FileCharacterRepository;

pub(crate) fn file_ctime_millis(metadata: &std::fs::Metadata) -> Option<i64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.ctime() * 1000 + metadata.ctime_nsec() / 1_000_000)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const WINDOWS_TICKS_TO_UNIX_EPOCH: u64 = 116444736000000000;
        let unix_ticks = metadata
            .creation_time()
            .checked_sub(WINDOWS_TICKS_TO_UNIX_EPOCH)?;
        Some((unix_ticks / 10_000) as i64)
    }

    #[cfg(not(any(unix, windows)))]
    {
        metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
    }
}

pub(crate) fn file_modified_millis(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

impl FileCharacterRepository {
    pub(crate) fn calculate_data_size(data: &Value) -> u64 {
        fn js_string_len(value: &Value) -> u64 {
            match value {
                Value::Null => 4,
                Value::Bool(value) => value.to_string().encode_utf16().count() as u64,
                Value::Number(value) => value.to_string().encode_utf16().count() as u64,
                Value::String(value) => value.encode_utf16().count() as u64,
                Value::Array(values) => {
                    values
                        .iter()
                        .map(|value| {
                            if value.is_null() {
                                0
                            } else {
                                js_string_len(value)
                            }
                        })
                        .sum::<u64>()
                        + values.len().saturating_sub(1) as u64
                }
                Value::Object(_) => "[object Object]".len() as u64,
            }
        }

        data.as_object()
            .into_iter()
            .flat_map(|data| data.values())
            .map(js_string_len)
            .sum()
    }

    pub(crate) fn calculate_character_data_size(card_value: &Value, character: &Character) -> u64 {
        if let Some(data) = card_value.get("data") {
            return Self::calculate_data_size(data);
        }

        let data = serde_json::to_value(&character.to_v2().data)
            .expect("CharacterData serialization should not fail");
        Self::calculate_data_size(&data)
    }

    pub(crate) fn normalize_character_file_stem(name: &str) -> Result<String, DomainError> {
        let normalized = sanitize_filename(name)
            .trim()
            .trim_end_matches(['.', ' '])
            .to_string();

        if normalized.is_empty() {
            return Err(DomainError::InvalidData(
                "Character name is invalid".to_string(),
            ));
        }

        Ok(normalized)
    }

    pub(crate) fn resolve_renamed_file_stem(
        &self,
        requested_name: &str,
        _current_file_stem: &str,
    ) -> Result<String, DomainError> {
        let base = Self::normalize_character_file_stem(requested_name)?;

        let mut candidate = base.clone();
        let mut suffix = 1usize;

        while self.get_character_path(&candidate).exists() {
            candidate = format!("{}{}", base, suffix);
            suffix += 1;
        }

        Ok(candidate)
    }

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

    pub(crate) fn chat_directory_for(chats_dir: &Path, name: &str) -> PathBuf {
        chats_dir.join(name)
    }

    pub(crate) fn get_chat_directory(&self, name: &str) -> PathBuf {
        Self::chat_directory_for(&self.chats_dir, name)
    }

    pub(crate) async fn resolve_chat_directory_for(
        characters_dir: &Path,
        chats_dir: &Path,
        chat_aliases: &SharedChatAliasStore,
        name: &str,
    ) -> Result<PathBuf, DomainError> {
        let dir_key = chat_directory_identity::resolve_character_chat_dir_key(
            characters_dir,
            chats_dir,
            chat_aliases,
            name,
        )
        .await?;
        Ok(Self::chat_directory_for(chats_dir, &dir_key))
    }

    pub(crate) async fn resolve_chat_directory(&self, name: &str) -> Result<PathBuf, DomainError> {
        Self::resolve_chat_directory_for(
            &self.characters_dir,
            &self.chats_dir,
            &self.chat_aliases,
            name,
        )
        .await
    }

    pub(crate) async fn calculate_chat_stats(&self, name: &str) -> (u64, i64) {
        Self::chat_stats_or_default(
            name,
            self.chat_repository
                .calculate_character_chat_stats(name)
                .await,
        )
    }

    pub(super) fn chat_stats_or_default(
        name: &str,
        result: Result<(u64, i64), DomainError>,
    ) -> (u64, i64) {
        result.unwrap_or_else(|error| {
            tracing::error!(
                target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Failed to calculate chat statistics for character '{}'; using zero statistics: {}",
                name,
                error
            );
            (0, 0)
        })
    }

    pub(crate) async fn read_character_from_file(
        &self,
        path: &Path,
    ) -> Result<Character, DomainError> {
        tracing::debug!("Reading character from file: {:?}", path);

        let metadata = fs::metadata(path).await.map_err(|e| {
            tracing::error!("Failed to read file metadata: {}", e);
            DomainError::InternalError(format!("Failed to read file metadata: {}", e))
        })?;
        let modified_millis = file_modified_millis(&metadata);
        let timestamp_millis = file_ctime_millis(&metadata)
            .or_else(|| (modified_millis > 0).then_some(modified_millis));

        let json_data = read_character_data_from_png_file(path).await?;

        let raw_value: Value = serde_json::from_str(&json_data).map_err(|e| {
            tracing::error!("Failed to parse character data: {}", e);
            DomainError::InvalidData(format!("Failed to parse character data: {}", e))
        })?;
        let mut character = Character::from_card_value(&raw_value).ok_or_else(|| {
            DomainError::InvalidData("Character payload must be a JSON object".to_string())
        })?;
        Self::sync_canonical_data_fields(&mut character, &raw_value);
        Self::normalize_imported_character(&mut character)?;
        let data_size = Self::calculate_character_data_size(&raw_value, &character);
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

        if let Some(timestamp_millis) = timestamp_millis {
            character.date_added = timestamp_millis;
        }

        character.json_data = Some(json_data);

        let (chat_size, date_last_chat) = self.calculate_chat_stats(&file_name).await;
        character.chat_size = chat_size;
        character.data_size = data_size;
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
                return Ok(character.into_shallow());
            }

            if !character.shallow {
                let mut character = character;
                let (chat_size, date_last_chat) = self.calculate_chat_stats(file_name).await;
                character.chat_size = chat_size;
                character.date_last_chat = date_last_chat;
                return Ok(character);
            }
        }

        let path = self.get_character_path(file_name);
        let character = self.read_character_from_file(&path).await?;
        let result = if shallow {
            character.into_shallow()
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
        if shallow {
            return self.load_shallow_character_index().await;
        }

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
                    tracing::error!(
                        target: tt_contracts::observability::USER_VISIBLE_ERROR,
                        "Failed to process character {}: {}",
                        file_name,
                        e
                    );
                }
            }
        }

        Ok(characters)
    }

    pub(crate) async fn list_avatar_filenames(&self) -> Result<Vec<String>, DomainError> {
        self.ensure_directory_exists().await?;

        let character_files = list_files_with_extension(&self.characters_dir, "png").await?;
        let mut avatars = Vec::with_capacity(character_files.len());

        for path in character_files {
            if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                avatars.push(file_name.to_string());
            } else {
                tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Skipping character avatar with a non-UTF-8 path: {:?}",
                    path
                );
            }
        }

        Ok(avatars)
    }

    pub(crate) async fn read_default_avatar(&self) -> Result<Vec<u8>, DomainError> {
        match fs::read(&self.default_avatar_path).await {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    "Default avatar not found at {:?}, using generated placeholder image",
                    self.default_avatar_path
                );
                Self::generate_placeholder_avatar_png()
            }
            Err(error) => {
                tracing::error!("Failed to read default avatar: {}", error);
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
