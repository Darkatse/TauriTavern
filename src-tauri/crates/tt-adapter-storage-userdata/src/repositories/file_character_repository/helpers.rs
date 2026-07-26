use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::Value;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tokio::fs;

use crate::png_card_metadata::{read_character_data_from_png, write_character_data_to_png};
use tt_adapter_storage_core::chat_directory_identity::{self, SharedChatAliasStore};
use tt_adapter_storage_core::file_system::{
    list_files_with_extension, replace_file_with_fallback, unique_temp_path,
};
use tt_domain::errors::DomainError;
use tt_domain::models::character::{Character, CharacterData};
use tt_domain::models::chat::parse_message_timestamp;
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
    pub(crate) fn calculate_data_size(data: &CharacterData) -> u64 {
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

        let value =
            serde_json::to_value(data).expect("CharacterData serialization should not fail");
        value
            .as_object()
            .expect("CharacterData should serialize to a JSON object")
            .values()
            .map(js_string_len)
            .sum()
    }

    fn is_valid_character_create_date(value: &str) -> bool {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return true;
        }

        if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
            return trimmed.parse::<i64>().is_ok();
        }

        if DateTime::parse_from_rfc3339(trimmed).is_ok() {
            return true;
        }

        parse_message_timestamp(trimmed) > 0
    }

    fn migrate_legacy_character_create_date_value(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S UTC") else {
            return None;
        };

        Some(
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        )
    }

    fn format_timestamp_millis(timestamp_millis: i64) -> Option<String> {
        Utc.timestamp_millis_opt(timestamp_millis)
            .single()
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    pub(crate) fn repaired_character_create_date(
        value: &str,
        fallback_timestamp_millis: Option<i64>,
    ) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        if Self::is_valid_character_create_date(trimmed) {
            return None;
        }

        if let Some(migrated) = Self::migrate_legacy_character_create_date_value(trimmed) {
            return Some(migrated);
        }

        if let Some(timestamp_millis) = fallback_timestamp_millis
            && let Some(formatted) = Self::format_timestamp_millis(timestamp_millis)
        {
            return Some(formatted);
        }

        Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true))
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

    pub(crate) async fn calculate_chat_stats(&self, name: &str) -> Result<(u64, i64), DomainError> {
        self.chat_repository
            .calculate_character_chat_stats(name)
            .await
    }

    pub(crate) async fn read_character_from_file(
        &self,
        path: &Path,
    ) -> Result<Character, DomainError> {
        tracing::debug!("Reading character from file: {:?}", path);

        let file_data = fs::read(path).await.map_err(|e| {
            tracing::error!("Failed to read character file: {}", e);
            DomainError::InternalError(format!("Failed to read character file: {}", e))
        })?;

        let metadata = fs::metadata(path).await.map_err(|e| {
            tracing::error!("Failed to read file metadata: {}", e);
            DomainError::InternalError(format!("Failed to read file metadata: {}", e))
        })?;
        let modified_millis = file_modified_millis(&metadata);
        let timestamp_millis = file_ctime_millis(&metadata)
            .or_else(|| (modified_millis > 0).then_some(modified_millis));

        let mut json_data = read_character_data_from_png(&file_data)?;

        let raw_value: Value = serde_json::from_str(&json_data).map_err(|e| {
            tracing::error!("Failed to parse character data: {}", e);
            DomainError::InvalidData(format!("Failed to parse character data: {}", e))
        })?;
        let mut character: Character = serde_json::from_value(raw_value.clone()).map_err(|e| {
            tracing::error!("Failed to decode character data: {}", e);
            DomainError::InvalidData(format!("Failed to decode character data: {}", e))
        })?;
        Self::sync_canonical_data_fields(&mut character, &raw_value);
        Self::normalize_imported_character(&mut character)?;
        let data_size = Self::calculate_data_size(&character.data);
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

        if let Some(repaired_create_date) =
            Self::repaired_character_create_date(&character.create_date, timestamp_millis)
        {
            tracing::warn!(
                character = %file_name,
                old_create_date = %character.create_date,
                new_create_date = %repaired_create_date,
                "Repairing invalid character create_date"
            );

            let mut card_value: Value = serde_json::from_str(&json_data).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Failed to parse character payload JSON in '{}': {}",
                    path.display(),
                    error
                ))
            })?;

            let Some(object) = card_value.as_object_mut() else {
                return Err(DomainError::InvalidData(format!(
                    "Character payload must be a JSON object in '{}'",
                    path.display()
                )));
            };

            object.insert(
                "create_date".to_string(),
                Value::String(repaired_create_date.clone()),
            );

            let updated_json = serde_json::to_string(&card_value).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to serialize repaired character payload for '{}': {}",
                    path.display(),
                    error
                ))
            })?;
            let updated_png = write_character_data_to_png(&file_data, &updated_json)?;

            let temp_path = unique_temp_path(path);
            fs::write(&temp_path, updated_png).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write repaired character temp file '{}': {}",
                    temp_path.display(),
                    error
                ))
            })?;
            replace_file_with_fallback(&temp_path, path).await?;
            self.clear_shallow_index_cache().await;

            character.create_date = repaired_create_date;
            json_data = updated_json;
        }

        character.json_data = Some(json_data);

        let (chat_size, date_last_chat) = self.calculate_chat_stats(&file_name).await?;
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
                let (chat_size, date_last_chat) = self.calculate_chat_stats(file_name).await?;
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
            let file_name = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Character avatar path is not valid UTF-8: {:?}",
                    path
                ))
            })?;
            avatars.push(file_name.to_string());
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
