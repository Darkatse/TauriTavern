use std::path::Path;

use serde_json::Value;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::chat::Chat;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::jsonl_utils::{
    parse_jsonl_bytes, read_first_non_empty_jsonl_line, write_jsonl_file,
};

use super::FileChatRepository;

impl FileChatRepository {
    pub(super) fn parse_chat_from_payload(
        &self,
        fallback_character_name: &str,
        file_name: &str,
        objects: &[Value],
    ) -> Result<Chat, DomainError> {
        if objects.is_empty() {
            return Err(DomainError::InvalidData("Empty JSONL file".to_string()));
        }

        let metadata = &objects[0];
        let user_name = metadata
            .get("user_name")
            .and_then(Value::as_str)
            .unwrap_or("User")
            .to_string();
        let character_name = metadata
            .get("character_name")
            .and_then(Value::as_str)
            .unwrap_or(fallback_character_name)
            .to_string();
        let create_date = metadata
            .get("create_date")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut chat = Chat {
            user_name,
            character_name,
            create_date,
            file_name: Some(Self::strip_jsonl_extension(file_name).to_string()),
            ..Default::default()
        };

        if let Some(chat_metadata) = metadata.get("chat_metadata") {
            chat.chat_metadata =
                serde_json::from_value(chat_metadata.clone()).map_err(|error| {
                    DomainError::InvalidData(format!(
                        "Failed to parse chat metadata for {}: {}",
                        file_name, error
                    ))
                })?;
        }

        for (index, obj) in objects.iter().enumerate().skip(1) {
            let message = serde_json::from_value(obj.clone()).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Failed to parse chat message at line {} for {}: {}",
                    index + 1,
                    file_name,
                    error
                ))
            })?;
            chat.add_message(message);
        }

        Ok(chat)
    }

    pub(super) fn build_payload_from_chat(chat: &Chat) -> Result<Vec<Value>, DomainError> {
        let mut objects = Vec::with_capacity(chat.messages.len() + 1);
        objects.push(serde_json::json!({
            "user_name": chat.user_name,
            "character_name": chat.character_name,
            "create_date": chat.create_date,
            "chat_metadata": chat.chat_metadata,
        }));

        for message in &chat.messages {
            objects.push(serde_json::to_value(message).map_err(|error| {
                DomainError::InternalError(format!("Failed to serialize chat message: {}", error))
            })?);
        }

        Ok(objects)
    }

    fn extract_integrity_slug_from_header(header: &Value) -> Option<String> {
        header
            .get("chat_metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("integrity"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
    }

    fn extract_integrity_slug_from_jsonl_line(line: &str) -> Result<Option<String>, DomainError> {
        let header: Value = serde_json::from_str(line).map_err(|error| {
            DomainError::InvalidData(format!("Failed to parse chat payload header: {}", error))
        })?;
        Ok(Self::extract_integrity_slug_from_header(&header))
    }

    async fn read_integrity_slug_from_existing_file(
        &self,
        path: &Path,
    ) -> Result<Option<String>, DomainError> {
        if !path.exists() {
            return Ok(None);
        }

        if let Some(line) = read_first_non_empty_jsonl_line(path).await? {
            return Self::extract_integrity_slug_from_jsonl_line(&line);
        }

        Ok(None)
    }

    async fn verify_chat_integrity_if_needed(
        &self,
        path: &Path,
        payload: &[Value],
        force: bool,
    ) -> Result<(), DomainError> {
        if force {
            return Ok(());
        }

        let Some(header) = payload.first() else {
            return Err(DomainError::InvalidData(
                "Chat payload is empty".to_string(),
            ));
        };

        let Some(incoming_integrity) = Self::extract_integrity_slug_from_header(header) else {
            return Ok(());
        };

        let existing_integrity = self.read_integrity_slug_from_existing_file(path).await?;
        if let Some(existing) = existing_integrity {
            if existing != incoming_integrity {
                return Err(DomainError::InvalidData("integrity".to_string()));
            }
        }

        Ok(())
    }

    async fn read_incoming_integrity_from_file(
        payload_path: &Path,
    ) -> Result<Option<String>, DomainError> {
        let Some(line) = read_first_non_empty_jsonl_line(payload_path).await? else {
            return Err(DomainError::InvalidData(
                "Chat payload is empty".to_string(),
            ));
        };

        Self::extract_integrity_slug_from_jsonl_line(&line)
    }

    async fn verify_chat_integrity_file_if_needed(
        &self,
        path: &Path,
        payload_path: &Path,
        force: bool,
    ) -> Result<(), DomainError> {
        if force {
            return Ok(());
        }

        let Some(incoming_integrity) =
            Self::read_incoming_integrity_from_file(payload_path).await?
        else {
            return Ok(());
        };

        let existing_integrity = self.read_integrity_slug_from_existing_file(path).await?;
        if let Some(existing) = existing_integrity {
            if existing != incoming_integrity {
                return Err(DomainError::InvalidData("integrity".to_string()));
            }
        }

        Ok(())
    }

    pub(super) async fn write_payload_to_path(
        &self,
        path: &Path,
        payload: &[Value],
        force: bool,
        backup_name: &str,
        backup_key: &str,
    ) -> Result<(), DomainError> {
        if payload.is_empty() {
            return Err(DomainError::InvalidData(
                "Chat payload is empty".to_string(),
            ));
        }

        self.verify_chat_integrity_if_needed(path, payload, force)
            .await?;
        write_jsonl_file(path, payload).await?;
        self.backup_chat_file(path, backup_name, backup_key).await?;

        Ok(())
    }

    pub(super) async fn write_payload_file_to_path(
        &self,
        path: &Path,
        source_path: &Path,
        force: bool,
        backup_name: &str,
        backup_key: &str,
    ) -> Result<(), DomainError> {
        if !source_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Chat payload source file not found: {:?}",
                source_path
            )));
        }

        self.verify_chat_integrity_file_if_needed(path, source_path, force)
            .await?;

        let temp_path = path.with_extension("jsonl.tmp");
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    DomainError::InternalError(format!("Failed to create directory: {}", e))
                })?;
            }
        }

        fs::copy(source_path, &temp_path).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to copy chat payload file: {}", e))
        })?;
        fs::rename(&temp_path, path).await.map_err(|e| {
            DomainError::InternalError(format!("Failed to move chat payload file: {}", e))
        })?;

        self.backup_chat_file(path, backup_name, backup_key).await?;
        Ok(())
    }

    pub(super) async fn read_payload_bytes_from_path(
        &self,
        path: &Path,
    ) -> Result<Vec<u8>, DomainError> {
        fs::read(path).await.map_err(|e| {
            DomainError::InternalError(format!(
                "Failed to read chat payload bytes {:?}: {}",
                path, e
            ))
        })
    }

    /// Read a chat from a file
    pub(super) async fn read_chat_file(
        &self,
        character_name: &str,
        file_name: &str,
    ) -> Result<Chat, DomainError> {
        logger::debug(&format!(
            "Reading chat file: {}/{}",
            character_name, file_name
        ));

        let file_name = Self::normalize_jsonl_file_name(file_name);

        let path = self.get_chat_path(character_name, &file_name);
        let bytes = self.read_payload_bytes_from_path(&path).await?;
        let objects: Vec<Value> = parse_jsonl_bytes(&bytes)?;
        self.parse_chat_from_payload(character_name, &file_name, &objects)
    }

    /// Write a chat to a file
    pub(super) async fn write_chat_file(
        &self,
        chat: &Chat,
        force: bool,
    ) -> Result<(), DomainError> {
        let file_name = chat
            .file_name
            .as_ref()
            .ok_or_else(|| DomainError::InvalidData("Chat file name is not set".to_string()))?;

        logger::debug(&format!(
            "Writing chat file: {}/{}",
            chat.character_name, file_name
        ));

        // Ensure the character directory exists
        let character_dir = self.get_character_dir(&chat.character_name);
        if !character_dir.exists() {
            fs::create_dir_all(&character_dir).await.map_err(|e| {
                logger::error(&format!("Failed to create character directory: {}", e));
                DomainError::InternalError(format!("Failed to create character directory: {}", e))
            })?;
        }

        let path = self.get_chat_path(&chat.character_name, file_name);
        let objects = Self::build_payload_from_chat(chat)?;
        let backup_key = self.get_cache_key(&chat.character_name, file_name);

        self.write_payload_to_path(&path, &objects, force, &chat.character_name, &backup_key)
            .await?;

        // Update cache
        let mut cache = self.memory_cache.lock().await;
        cache.set(backup_key, chat.clone());

        Ok(())
    }
}
