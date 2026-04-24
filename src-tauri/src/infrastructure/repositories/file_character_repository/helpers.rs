use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::Value;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::character::{Character, sanitize_filename};
use crate::domain::models::chat::parse_message_timestamp;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::file_system::{
    list_files_with_extension, replace_file_with_fallback, unique_temp_path,
};
use crate::infrastructure::persistence::png_utils::{
    read_character_data_from_png, write_character_data_to_png,
};

use super::FileCharacterRepository;

fn file_ctime_millis(metadata: &std::fs::Metadata) -> Option<i64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return Some(metadata.ctime() * 1000 + metadata.ctime_nsec() / 1_000_000);
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const WINDOWS_TICKS_TO_UNIX_EPOCH: u64 = 116444736000000000;
        let unix_ticks = metadata
            .creation_time()
            .checked_sub(WINDOWS_TICKS_TO_UNIX_EPOCH)?;
        return Some((unix_ticks / 10_000) as i64);
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

impl FileCharacterRepository {
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

    fn repaired_character_create_date(
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

        if let Some(timestamp_millis) = fallback_timestamp_millis {
            if let Some(formatted) = Self::format_timestamp_millis(timestamp_millis) {
                return Some(formatted);
            }
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

        let metadata = fs::metadata(path).await.map_err(|e| {
            logger::error(&format!("Failed to read file metadata: {}", e));
            DomainError::InternalError(format!("Failed to read file metadata: {}", e))
        })?;
        let timestamp_millis = file_ctime_millis(&metadata);

        let mut json_data = read_character_data_from_png(&file_data)?;

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

            let temp_path = unique_temp_path(path, "character.png");
            fs::write(&temp_path, updated_png).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write repaired character temp file '{}': {}",
                    temp_path.display(),
                    error
                ))
            })?;
            replace_file_with_fallback(&temp_path, path).await?;

            character.create_date = repaired_create_date;
            json_data = updated_json;
        }

        character.json_data = Some(json_data);

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
