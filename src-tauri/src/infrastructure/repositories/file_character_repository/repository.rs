use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::domain::json_merge::merge_json_value;
use crate::infrastructure::persistence::png_utils::{
    process_avatar_image, read_character_data_from_png, write_character_data_to_png,
};
use crate::infrastructure::persistence::thumbnail_cache::invalidate_thumbnail_cache;
use tt_domain::errors::DomainError;
use tt_domain::models::character::Character;
use tt_domain::models::chat::parse_message_timestamp_value;
use tt_ports::repositories::character_repository::{
    CHARACTER_CREATE_WARNING_AVATAR_IMPORT_FAILED, CharacterChat, CharacterCreateResult,
    CharacterCreateWarning, CharacterRepository, ImageCrop,
};

use super::FileCharacterRepository;

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
        stored
    }

    async fn invalidate_avatar_thumbnail(&self, file_name: &str) -> Result<(), DomainError> {
        let thumbnail_path = self
            .thumbnails_avatar_dir
            .join(format!("{}.png", file_name));
        invalidate_thumbnail_cache(&thumbnail_path).await
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

        if preserve_existing_character_book_when_unbound && character.data.character_book.is_none()
        {
            if let Some(data_object) = projection
                .get_mut("data")
                .and_then(serde_json::Value::as_object_mut)
            {
                data_object.remove("character_book");
            }
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

        fs::write(&file_path, new_image_data).await.map_err(|e| {
            tracing::error!("Failed to write character file: {}", e);
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        let cached_character =
            Self::with_storage_identity_and_json(character, &file_name, Some(json_data));

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

        fs::write(&file_path, new_image_data).await.map_err(|e| {
            tracing::error!("Failed to write character file: {}", e);
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        if replaced_avatar {
            self.invalidate_avatar_thumbnail(name).await?;
        }

        let character = self.read_character_from_file(&file_path).await?;
        let mut cache = self.memory_cache.lock().await;
        cache.set(name.to_string(), character.clone());

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

        fs::write(&new_path, new_image_data).await.map_err(|e| {
            tracing::error!("Failed to write character file: {}", e);
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        let old_chat_dir = self.resolve_chat_directory(old_name).await?;
        let new_chat_dir = self.get_chat_directory(&target_file_stem);

        if old_chat_dir.exists() && old_chat_dir != new_chat_dir && !new_chat_dir.exists() {
            fs::rename(&old_chat_dir, &new_chat_dir)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to rename chat directory: {}", e);
                    DomainError::InternalError(format!("Failed to rename chat directory: {}", e))
                })?;
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
        let mut cache = self.memory_cache.lock().await;
        cache.set(target_file_stem, character.clone());

        Ok(character)
    }

    async fn import_character(
        &self,
        file_path: &Path,
        preserve_file_name: Option<String>,
    ) -> Result<Character, DomainError> {
        self.ensure_directory_exists().await?;

        let file_data = fs::read(file_path).await.map_err(|e| {
            tracing::error!("Failed to read file: {}", e);
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

        fs::write(&file_path, new_image_data).await.map_err(|e| {
            tracing::error!("Failed to write character file: {}", e);
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        let stored_character =
            Self::with_storage_identity_and_json(character, &file_name, Some(json_data));

        let mut cache = self.memory_cache.lock().await;
        cache.set(file_name, stored_character.clone());

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

        fs::write(&file_path, new_image_data).await.map_err(|e| {
            tracing::error!("Failed to write character file: {}", e);
            DomainError::InternalError(format!("Failed to write character file: {}", e))
        })?;

        self.invalidate_avatar_thumbnail(&file_name).await?;

        let cached_character =
            Self::with_storage_identity_and_json(character, &file_name, Some(json_data));
        let mut cache = self.memory_cache.lock().await;
        cache.set(file_name, cached_character);

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
