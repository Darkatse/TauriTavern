use std::{io::SeekFrom, path::Path};

use async_trait::async_trait;
use serde_json::Value;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt},
};

use crate::png_card_metadata::{
    process_avatar_image, read_character_data_from_png, write_character_data_to_png,
};
use tt_adapter_storage_core::file_system::{replace_file_with_fallback, unique_temp_path};
use tt_contracts::client_asset_paths::validate_path_segment;
use tt_domain::errors::DomainError;
use tt_domain::json_merge::merge_json_value;
use tt_domain::models::{character::Character, chat::parse_message_timestamp_value};
use tt_ports::repositories::character_repository::{
    CHARACTER_CREATE_WARNING_AVATAR_IMPORT_FAILED, CharacterChat, CharacterCreateResult,
    CharacterCreateWarning, CharacterRepository, ImageCrop,
};
use tt_ports::repositories::chat_repository::{ChatRepository, ChatSearchResult};

use super::{FileCharacterRepository, importer::CharacterImportMode};

const CHARACTER_CHAT_TAIL_SCAN_BUFFER_BYTES: usize = 64 * 1024;

struct CreateAvatarCarrier {
    image_data: Vec<u8>,
    can_fallback_to_default: bool,
    warnings: Vec<CharacterCreateWarning>,
}

fn is_png_bytes(image_data: &[u8]) -> bool {
    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    image_data.starts_with(&PNG_SIGNATURE)
}

fn avatar_import_warning(message: impl Into<String>) -> CharacterCreateWarning {
    CharacterCreateWarning {
        code: CHARACTER_CREATE_WARNING_AVATAR_IMPORT_FAILED.to_string(),
        message: message.into(),
    }
}

impl FileCharacterRepository {
    async fn replace_character_png_file(
        file_path: &Path,
        image_data: &[u8],
    ) -> Result<(), DomainError> {
        let temp_path = unique_temp_path(file_path);
        fs::write(&temp_path, image_data).await.map_err(|error| {
            tracing::error!(
                "Failed to write character temp file {}: {}",
                temp_path.display(),
                error
            );
            DomainError::InternalError(format!("Failed to write character temp file: {error}"))
        })?;
        replace_file_with_fallback(&temp_path, file_path).await
    }

    fn with_storage_identity_and_json(
        character: &Character,
        file_name: &str,
        json_data: Option<String>,
    ) -> Character {
        let mut stored = character.clone();
        stored.file_name = Some(file_name.to_string());
        stored.avatar = format!("{}.png", file_name);
        stored.json_data = json_data;
        stored.shallow = false;
        stored.data_size = Self::calculate_data_size(&stored.to_v2().data);
        stored
    }

    pub(crate) async fn discard_character_read_cache(&self, file_name: &str) {
        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(file_name);
        }
        self.clear_shallow_index_cache().await;
    }

    pub(crate) async fn publish_character_write(&self, file_name: String, character: Character) {
        {
            let mut cache = self.memory_cache.lock().await;
            cache.set(file_name, character);
        }
        self.clear_shallow_index_cache().await;
    }

    async fn default_create_avatar_carrier(&self) -> Result<CreateAvatarCarrier, DomainError> {
        Ok(CreateAvatarCarrier {
            image_data: self.read_default_avatar().await?,
            can_fallback_to_default: false,
            warnings: Vec::new(),
        })
    }

    async fn resolve_create_avatar_carrier(
        &self,
        avatar_path: Option<&Path>,
        crop: Option<ImageCrop>,
    ) -> Result<CreateAvatarCarrier, DomainError> {
        let Some(path) = avatar_path else {
            return self.default_create_avatar_carrier().await;
        };

        let file_data = match fs::read(path).await {
            Ok(file_data) => file_data,
            Err(error) => {
                tracing::warn!(
                    "Failed to read avatar file for character create {}: {}. Using default avatar.",
                    path.display(),
                    error
                );
                let mut carrier = self.default_create_avatar_carrier().await?;
                carrier.warnings.push(avatar_import_warning(
                    "Uploaded avatar could not be read; default avatar was used.",
                ));
                return Ok(carrier);
            }
        };

        if crop.is_none() && is_png_bytes(&file_data) {
            return Ok(CreateAvatarCarrier {
                image_data: file_data,
                can_fallback_to_default: true,
                warnings: Vec::new(),
            });
        }

        let raw_png_candidate = is_png_bytes(&file_data).then(|| file_data.clone());

        match process_avatar_image(file_data, crop).await {
            Ok(image_data) => Ok(CreateAvatarCarrier {
                image_data,
                can_fallback_to_default: true,
                warnings: Vec::new(),
            }),
            Err(error) => {
                let Some(image_data) = raw_png_candidate else {
                    tracing::warn!(
                        "Failed to process avatar file for character create {}: {}. Using default avatar.",
                        path.display(),
                        error
                    );
                    let mut carrier = self.default_create_avatar_carrier().await?;
                    carrier.warnings.push(avatar_import_warning(
                        "Uploaded avatar could not be processed; default avatar was used.",
                    ));
                    return Ok(carrier);
                };

                tracing::warn!(
                    "Failed to process avatar file for character create {}: {}. Trying raw PNG bytes before default avatar fallback.",
                    path.display(),
                    error
                );
                Ok(CreateAvatarCarrier {
                    image_data,
                    can_fallback_to_default: true,
                    warnings: vec![avatar_import_warning(
                        "Uploaded avatar could not be processed; original PNG bytes were used.",
                    )],
                })
            }
        }
    }

    async fn write_create_character_png(
        &self,
        mut carrier: CreateAvatarCarrier,
        json_data: &str,
    ) -> Result<(Vec<u8>, Vec<CharacterCreateWarning>), DomainError> {
        match write_character_data_to_png(&carrier.image_data, json_data) {
            Ok(image_data) => Ok((image_data, carrier.warnings)),
            Err(error) if carrier.can_fallback_to_default => {
                tracing::warn!(
                    "Failed to write character metadata to uploaded avatar: {}. Using default avatar.",
                    error
                );
                let default_avatar = self.read_default_avatar().await?;
                carrier.warnings.push(avatar_import_warning(
                    "Uploaded avatar could not store character data; default avatar was used.",
                ));
                let image_data = write_character_data_to_png(&default_avatar, json_data)?;
                Ok((image_data, carrier.warnings))
            }
            Err(error) => Err(error),
        }
    }

    fn next_duplicate_file_stem(&self, source_file_stem: &str) -> Result<String, DomainError> {
        let source_file_stem = Self::normalize_character_file_stem(source_file_stem)?;
        let (base, mut suffix) = if let Some((base, suffix)) = source_file_stem.rsplit_once('_') {
            if !base.is_empty() {
                match suffix.parse::<usize>() {
                    Ok(value) => (base.to_string(), value + 1),
                    Err(_) => (source_file_stem.clone(), 1),
                }
            } else {
                (source_file_stem.clone(), 1)
            }
        } else {
            (source_file_stem.clone(), 1)
        };

        loop {
            let candidate = format!("{}_{}", base, suffix);
            if !self.get_character_path(&candidate).exists() {
                return Ok(candidate);
            }
            suffix += 1;
        }
    }

    async fn character_chat_from_summary(
        chat_dir: &Path,
        summary: ChatSearchResult,
    ) -> Result<CharacterChat, DomainError> {
        let path = chat_dir.join(&summary.file_name);
        let (last_message, last_message_date) =
            Self::read_last_message_from_chat_file(&path, summary.date).await?;

        Ok(CharacterChat {
            file_name: summary.file_name,
            file_size: format!("{:.2}kb", summary.file_size as f64 / 1024.0),
            chat_items: summary.message_count,
            last_message,
            last_message_date,
        })
    }

    async fn read_last_message_from_chat_file(
        path: &Path,
        fallback_date: i64,
    ) -> Result<(String, i64), DomainError> {
        let Some(line) = Self::read_last_non_empty_chat_line(path).await? else {
            return Ok(("[The chat is empty]".to_string(), fallback_date));
        };

        let json = match serde_json::from_slice::<Value>(&line) {
            Ok(json) => json,
            Err(_) => return Ok(("[Invalid chat format]".to_string(), fallback_date)),
        };

        let message = json
            .get("mes")
            .and_then(Value::as_str)
            .unwrap_or("[The chat is empty]")
            .to_string();
        let parsed_date = parse_message_timestamp_value(json.get("send_date"));
        let last_message_date = if parsed_date > 0 {
            parsed_date
        } else {
            fallback_date
        };

        Ok((message, last_message_date))
    }

    async fn read_last_non_empty_chat_line(path: &Path) -> Result<Option<Vec<u8>>, DomainError> {
        let mut file = fs::File::open(path).await.map_err(|e| {
            DomainError::InternalError(format!(
                "Failed to open chat file '{}': {}",
                path.display(),
                e
            ))
        })?;
        let metadata = file.metadata().await.map_err(|e| {
            DomainError::InternalError(format!(
                "Failed to read chat metadata '{}': {}",
                path.display(),
                e
            ))
        })?;

        let mut position = metadata.len();
        if position == 0 {
            return Ok(None);
        }

        let mut reversed_line = Vec::new();
        while position > 0 {
            let read_len = position.min(CHARACTER_CHAT_TAIL_SCAN_BUFFER_BYTES as u64) as usize;
            position -= read_len as u64;
            file.seek(SeekFrom::Start(position)).await.map_err(|e| {
                DomainError::InternalError(format!(
                    "Failed to seek chat file '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            let mut buffer = vec![0_u8; read_len];
            file.read_exact(&mut buffer).await.map_err(|e| {
                DomainError::InternalError(format!(
                    "Failed to read chat file '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            for &byte in buffer.iter().rev() {
                if byte == b'\n' {
                    if let Some(line) = Self::finish_reversed_chat_line(path, &mut reversed_line)? {
                        return Ok(Some(line));
                    }
                } else {
                    reversed_line.push(byte);
                }
            }
        }

        Self::finish_reversed_chat_line(path, &mut reversed_line)
    }

    fn finish_reversed_chat_line(
        path: &Path,
        reversed_line: &mut Vec<u8>,
    ) -> Result<Option<Vec<u8>>, DomainError> {
        if reversed_line.is_empty() {
            return Ok(None);
        }

        reversed_line.reverse();
        let mut line = std::mem::take(reversed_line);
        Self::trim_trailing_carriage_returns(&mut line);
        let text = std::str::from_utf8(&line).map_err(|e| {
            DomainError::InternalError(format!(
                "Failed to decode chat line '{}': {}",
                path.display(),
                e
            ))
        })?;

        if text.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(line))
        }
    }

    fn trim_trailing_carriage_returns(line: &mut Vec<u8>) {
        while line.last() == Some(&b'\r') {
            line.pop();
        }
    }

    pub(super) fn parse_card_json(json_data: &str, context: &str) -> Result<Value, DomainError> {
        let value: Value = serde_json::from_str(json_data)
            .map_err(|e| DomainError::InvalidData(format!("Failed to parse {}: {}", context, e)))?;

        if !value.is_object() {
            return Err(DomainError::InvalidData(format!(
                "{} must be a JSON object",
                context
            )));
        }

        Ok(value)
    }

    pub(super) fn serialize_card_value(
        card_value: &Value,
        context: &str,
    ) -> Result<String, DomainError> {
        serde_json::to_string(card_value).map_err(|e| {
            DomainError::InvalidData(format!("Failed to serialize {}: {}", context, e))
        })
    }

    pub(super) fn serialize_character_card(character: &Character) -> Result<String, DomainError> {
        serde_json::to_string(&character.to_v2()).map_err(|e| {
            DomainError::InvalidData(format!("Failed to serialize character card: {}", e))
        })
    }

    fn character_projection_value(
        character: &Character,
        preserve_existing_spec: bool,
        preserve_existing_character_book_when_unbound: bool,
    ) -> Result<Value, DomainError> {
        let mut projection = serde_json::to_value(character.to_v2()).map_err(|e| {
            DomainError::InvalidData(format!("Failed to serialize character projection: {}", e))
        })?;

        let Some(projection_object) = projection.as_object_mut() else {
            return Err(DomainError::InvalidData(
                "Character projection must be a JSON object".to_string(),
            ));
        };

        if preserve_existing_spec {
            projection_object.remove("spec");
            projection_object.remove("spec_version");
        }

        if preserve_existing_character_book_when_unbound
            && character.data.character_book.is_none()
            && let Some(data_object) = projection
                .get_mut("data")
                .and_then(serde_json::Value::as_object_mut)
        {
            data_object.remove("character_book");
        }

        Ok(projection)
    }

    fn merge_character_projection_into_card_value_with_options(
        card_value: &mut Value,
        character: &Character,
        preserve_existing_spec: bool,
        preserve_existing_character_book_when_unbound: bool,
    ) -> Result<(), DomainError> {
        let projection = Self::character_projection_value(
            character,
            preserve_existing_spec,
            preserve_existing_character_book_when_unbound,
        )?;

        merge_json_value(card_value, projection);

        let Some(card_object) = card_value.as_object_mut() else {
            return Err(DomainError::InvalidData(
                "Character card payload must be a JSON object".to_string(),
            ));
        };
        card_object.remove("json_data");

        Ok(())
    }

    pub(super) fn merge_existing_character_projection_into_card_value(
        card_value: &mut Value,
        character: &Character,
    ) -> Result<(), DomainError> {
        Self::merge_character_projection_into_card_value_with_options(
            card_value, character, true, false,
        )
    }

    fn merge_existing_character_projection_into_card_json(
        json_data: &str,
        character: &Character,
        context: &str,
    ) -> Result<String, DomainError> {
        let mut card_value = Self::parse_card_json(json_data, context)?;
        Self::merge_existing_character_projection_into_card_value(&mut card_value, character)?;
        Self::serialize_card_value(&card_value, context)
    }

    fn merge_create_character_projection_into_card_json(
        json_data: &str,
        character: &Character,
        context: &str,
    ) -> Result<String, DomainError> {
        let mut card_value = Self::parse_card_json(json_data, context)?;
        Self::merge_character_projection_into_card_value_with_options(
            &mut card_value,
            character,
            false,
            true,
        )?;
        Self::serialize_card_value(&card_value, context)
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
                tracing::error!("Failed to read character file: {}", e);
                DomainError::InternalError(format!("Failed to read character file: {}", e))
            })?
        } else {
            self.read_default_avatar().await?
        };

        let json_data = if file_path.exists() {
            let raw_json = read_character_data_from_png(&image_data)?;
            Self::merge_existing_character_projection_into_card_json(
                &raw_json,
                character,
                "stored character card",
            )?
        } else {
            Self::serialize_character_card(character)?
        };

        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        Self::replace_character_png_file(&file_path, &new_image_data).await?;

        let cached_character =
            Self::with_storage_identity_and_json(character, &file_name, Some(json_data));

        self.publish_character_write(file_name, cached_character)
            .await;

        Ok(())
    }

    async fn find_by_name(&self, name: &str) -> Result<Character, DomainError> {
        let cached = {
            let cache = self.memory_cache.lock().await;
            cache.get(name)
        };

        if let Some(character) = cached
            && !character.shallow
        {
            return Ok(character);
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

    async fn list_avatar_filenames(&self) -> Result<Vec<String>, DomainError> {
        self.list_avatar_filenames().await
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
            tracing::error!("Failed to delete character file: {}", e);
            DomainError::InternalError(format!("Failed to delete character file: {}", e))
        })?;

        if delete_chats {
            let chat_dir = self.resolve_chat_directory(name).await?;
            if chat_dir.exists() {
                fs::remove_dir_all(&chat_dir).await.map_err(|e| {
                    tracing::error!("Failed to delete chat directory: {}", e);
                    DomainError::InternalError(format!("Failed to delete chat directory: {}", e))
                })?;
            }
            self.chat_repository
                .invalidate_all_payload_signatures()
                .await;
            self.chat_repository.clear_chat_summary_index().await?;
        }

        {
            let mut cache = self.memory_cache.lock().await;
            cache.remove(name);
        }
        self.clear_shallow_index_cache().await;

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

    async fn write_character_card_json(
        &self,
        name: &str,
        character_card_json: &str,
        avatar_path: Option<&Path>,
        crop: Option<ImageCrop>,
    ) -> Result<Character, DomainError> {
        let file_path = self.get_character_path(name);

        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                name
            )));
        }

        let replaced_avatar = avatar_path.is_some();
        let image_data = if let Some(avatar_path) = avatar_path {
            let file_data = fs::read(avatar_path).await.map_err(|e| {
                tracing::error!("Failed to read avatar file: {}", e);
                DomainError::InternalError(format!("Failed to read avatar file: {}", e))
            })?;

            process_avatar_image(file_data, crop).await?
        } else {
            fs::read(&file_path).await.map_err(|e| {
                tracing::error!("Failed to read character file: {}", e);
                DomainError::InternalError(format!("Failed to read character file: {}", e))
            })?
        };

        let new_image_data = write_character_data_to_png(&image_data, character_card_json)?;
        if !replaced_avatar && new_image_data == image_data {
            let character = self.read_character_from_file(&file_path).await?;
            let mut cache = self.memory_cache.lock().await;
            cache.set(name.to_string(), character.clone());
            return Ok(character);
        }

        if let Err(error) = Self::replace_character_png_file(&file_path, &new_image_data).await {
            {
                let mut cache = self.memory_cache.lock().await;
                cache.remove(name);
            }
            self.clear_shallow_index_cache().await;
            return Err(error);
        }

        let character = self.read_character_from_file(&file_path).await?;
        self.publish_character_write(name.to_string(), character.clone())
            .await;

        Ok(character)
    }

    async fn rename(&self, old_name: &str, new_name: &str) -> Result<Character, DomainError> {
        self.ensure_directory_exists().await?;

        let old_path = self.get_character_path(old_name);
        if !old_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                old_name
            )));
        }

        let new_name = new_name.trim();
        let target_file_stem = self.resolve_renamed_file_stem(new_name, old_name)?;
        let new_path = self.get_character_path(&target_file_stem);

        let old_image_data = fs::read(&old_path).await.map_err(|e| {
            tracing::error!("Failed to read character file: {}", e);
            DomainError::InternalError(format!("Failed to read character file: {}", e))
        })?;

        let card_json = read_character_data_from_png(&old_image_data)?;
        let mut card_value: serde_json::Value = serde_json::from_str(&card_json).map_err(|e| {
            tracing::error!("Failed to parse character data: {}", e);
            DomainError::InvalidData(format!("Failed to parse character data: {}", e))
        })?;

        let card_object = card_value.as_object_mut().ok_or_else(|| {
            DomainError::InvalidData("Character card data is not a JSON object".to_string())
        })?;

        card_object.insert(
            "name".to_string(),
            serde_json::Value::String(new_name.to_string()),
        );

        let data_value = card_object
            .entry("data")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

        let data_object = data_value.as_object_mut().ok_or_else(|| {
            DomainError::InvalidData("Character card data field is invalid".to_string())
        })?;

        data_object.insert(
            "name".to_string(),
            serde_json::Value::String(new_name.to_string()),
        );

        let patched_json = serde_json::to_string(&card_value).map_err(|e| {
            tracing::error!("Failed to serialize character data: {}", e);
            DomainError::InvalidData(format!("Failed to serialize character data: {}", e))
        })?;

        let new_image_data = write_character_data_to_png(&old_image_data, &patched_json)?;

        Self::replace_character_png_file(&new_path, &new_image_data).await?;

        let old_chat_dir = self.resolve_chat_directory(old_name).await?;
        let new_chat_dir = self.get_chat_directory(&target_file_stem);

        if old_chat_dir.exists() && old_chat_dir != new_chat_dir && !new_chat_dir.exists() {
            fs::rename(&old_chat_dir, &new_chat_dir)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to rename chat directory: {}", e);
                    DomainError::InternalError(format!("Failed to rename chat directory: {}", e))
                })?;
            self.chat_repository
                .invalidate_all_payload_signatures()
                .await;
            self.chat_repository.clear_chat_summary_index().await?;
        }

        if old_path != new_path {
            fs::remove_file(&old_path).await.map_err(|e| {
                tracing::error!("Failed to delete old character file: {}", e);
                DomainError::InternalError(format!("Failed to delete old character file: {}", e))
            })?;
        }

        let remove_old_cache_entry = old_name != target_file_stem;
        let character = self.read_character_from_file(&new_path).await?;
        {
            let mut cache = self.memory_cache.lock().await;
            cache.set(target_file_stem.clone(), character.clone());
            if remove_old_cache_entry {
                cache.remove(old_name);
            }
        }
        self.clear_shallow_index_cache().await;

        Ok(character)
    }

    async fn duplicate(&self, name: &str) -> Result<Character, DomainError> {
        self.ensure_directory_exists().await?;

        let source_file_stem = Self::normalize_character_file_stem(name)?;
        let source_path = self.get_character_path(&source_file_stem);
        if !source_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                source_file_stem
            )));
        }

        let target_file_stem = self.next_duplicate_file_stem(&source_file_stem)?;
        let target_path = self.get_character_path(&target_file_stem);

        fs::copy(&source_path, &target_path).await.map_err(|e| {
            tracing::error!("Failed to duplicate character file: {}", e);
            DomainError::InternalError(format!("Failed to duplicate character file: {}", e))
        })?;

        let character = self.read_character_from_file(&target_path).await?;
        self.publish_character_write(target_file_stem, character.clone())
            .await;

        Ok(character)
    }

    async fn import_character(
        &self,
        file_path: &Path,
        preserve_file_name: Option<String>,
    ) -> Result<Character, DomainError> {
        self.ensure_directory_exists().await?;
        let character = self
            .import_character_file(
                file_path,
                CharacterImportMode::New {
                    preserve_file_name: preserve_file_name.as_deref(),
                },
            )
            .await?;

        let file_name = character.get_file_name();
        self.publish_character_write(file_name.clone(), character.clone())
            .await;
        Ok(character)
    }

    async fn replace_character(
        &self,
        file_path: &Path,
        name: &str,
        primary_lorebook: Option<&str>,
    ) -> Result<Character, DomainError> {
        self.ensure_directory_exists().await?;
        if !validate_path_segment(name) {
            return Err(DomainError::InvalidData(
                "Character storage identity is invalid".to_string(),
            ));
        }
        if !self.get_character_path(name).exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                name
            )));
        }

        let character = self
            .import_character_file(
                file_path,
                CharacterImportMode::Replace {
                    file_stem: name,
                    primary_lorebook,
                },
            )
            .await?;
        self.publish_character_write(name.to_string(), character.clone())
            .await;
        Ok(character)
    }

    async fn export_character(
        &self,
        name: &str,
        target_path: &Path,
        character_card_json: &str,
    ) -> Result<(), DomainError> {
        let extension = target_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match extension.as_str() {
            "png" => {
                let png_bytes = self
                    .export_character_png_bytes(name, character_card_json)
                    .await?;
                fs::write(target_path, png_bytes).await.map_err(|error| {
                    tracing::error!("Failed to write exported character PNG: {}", error);
                    DomainError::InternalError(format!(
                        "Failed to write exported character PNG: {}",
                        error
                    ))
                })?;
                Ok(())
            }
            "json" => {
                fs::write(target_path, character_card_json.as_bytes())
                    .await
                    .map_err(|error| {
                        tracing::error!("Failed to write exported character JSON: {}", error);
                        DomainError::InternalError(format!(
                            "Failed to write exported character JSON: {}",
                            error
                        ))
                    })?;
                Ok(())
            }
            _ => Err(DomainError::InvalidData(format!(
                "Unsupported file format: {}",
                extension
            ))),
        }
    }

    async fn read_character_card_json(&self, name: &str) -> Result<String, DomainError> {
        let file_path = self.get_character_path(name);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                name
            )));
        }

        let image_data = fs::read(&file_path).await.map_err(|error| {
            tracing::error!(
                "Failed to read character file {}: {}",
                file_path.display(),
                error
            );
            DomainError::InternalError(format!("Failed to read character file: {}", error))
        })?;

        read_character_data_from_png(&image_data)
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
            tracing::error!(
                "Failed to read character file for export {}: {}",
                file_path.display(),
                e
            );
            DomainError::InternalError(format!("Failed to read character file: {}", e))
        })?;

        write_character_data_to_png(&image_data, character_card_json)
    }

    async fn create_with_avatar(
        &self,
        character: &Character,
        avatar_path: Option<&Path>,
        crop: Option<ImageCrop>,
    ) -> Result<CharacterCreateResult, DomainError> {
        self.ensure_directory_exists().await?;

        let avatar_carrier = self
            .resolve_create_avatar_carrier(avatar_path, crop)
            .await?;

        let json_data = match character.json_data.as_deref() {
            Some(raw_json) if !raw_json.trim().is_empty() => {
                Self::merge_create_character_projection_into_card_json(
                    raw_json,
                    character,
                    "character create json_data",
                )?
            }
            _ => Self::serialize_character_card(character)?,
        };

        let (new_image_data, warnings) = self
            .write_create_character_png(avatar_carrier, &json_data)
            .await?;

        let base = Self::normalize_character_file_stem(&character.get_file_name())?;
        let file_name = self.ensure_unique_file_stem(&base);
        let file_path = self.get_character_path(&file_name);

        Self::replace_character_png_file(&file_path, &new_image_data).await?;

        let stored_character =
            Self::with_storage_identity_and_json(character, &file_name, Some(json_data));

        self.publish_character_write(file_name, stored_character.clone())
            .await;

        Ok(CharacterCreateResult {
            character: stored_character,
            warnings,
        })
    }

    async fn update_avatar(
        &self,
        character: &Character,
        avatar_path: &Path,
        crop: Option<ImageCrop>,
    ) -> Result<(), DomainError> {
        let file_name = character.get_file_name();
        let file_path = self.get_character_path(&file_name);
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Character not found: {}",
                file_name
            )));
        }

        let existing_image_data = fs::read(&file_path).await.map_err(|e| {
            tracing::error!("Failed to read character file: {}", e);
            DomainError::InternalError(format!("Failed to read character file: {}", e))
        })?;
        let raw_json = read_character_data_from_png(&existing_image_data)?;
        let json_data = Self::merge_existing_character_projection_into_card_json(
            &raw_json,
            character,
            "stored character card",
        )?;

        let file_data = fs::read(avatar_path).await.map_err(|e| {
            tracing::error!("Failed to read avatar file: {}", e);
            DomainError::InternalError(format!("Failed to read avatar file: {}", e))
        })?;
        let image_data = process_avatar_image(file_data, crop).await?;
        let new_image_data = write_character_data_to_png(&image_data, &json_data)?;

        Self::replace_character_png_file(&file_path, &new_image_data).await?;

        let cached_character =
            Self::with_storage_identity_and_json(character, &file_name, Some(json_data));
        self.publish_character_write(file_name.clone(), cached_character)
            .await;
        Ok(())
    }

    async fn get_character_chats(
        &self,
        name: &str,
        simple: bool,
    ) -> Result<Vec<CharacterChat>, DomainError> {
        let chat_dir = self.resolve_chat_directory(name).await?;

        if !chat_dir.exists() {
            return Ok(Vec::new());
        }

        if simple {
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
                chats.push(CharacterChat {
                    file_name,
                    file_size: "".to_string(),
                    chat_items: 0,
                    last_message: "".to_string(),
                    last_message_date: 0,
                });
            }

            return Ok(chats);
        }

        let summaries = self
            .chat_repository
            .list_chat_summaries(Some(name), false)
            .await?;

        let mut chats = Vec::with_capacity(summaries.len());
        for summary in summaries {
            chats.push(Self::character_chat_from_summary(&chat_dir, summary).await?);
        }

        Ok(chats)
    }

    async fn clear_cache(&self) -> Result<(), DomainError> {
        {
            let mut cache = self.memory_cache.lock().await;
            cache.clear();
        }
        self.clear_shallow_index_cache().await;
        Ok(())
    }
}
