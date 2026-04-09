use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::json_merge::merge_json_value;
use crate::infrastructure::persistence::file_system::{
    replace_file_with_fallback, unique_temp_path,
};

use super::FileChatRepository;
use super::windowed_payload_io::read_first_line_and_end_offset;

fn validate_store_component(raw: &str, label: &str) -> Result<String, DomainError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(format!(
            "Chat store {} cannot be empty",
            label
        )));
    }

    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(DomainError::InvalidData(format!(
            "Chat store {} contains illegal characters",
            label
        )));
    }

    Ok(value.to_string())
}

fn extract_integrity_slug_from_header_value(header: &Value) -> Result<String, DomainError> {
    let meta = header
        .get("chat_metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            DomainError::InvalidData("Chat header is missing chat_metadata".to_string())
        })?;

    let slug = meta
        .get("integrity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            DomainError::InvalidData("Chat metadata integrity is missing".to_string())
        })?;

    Ok(slug.to_string())
}

async fn read_chat_integrity_slug(path: &Path) -> Result<String, DomainError> {
    let (header, _) = read_first_line_and_end_offset(path).await?;
    let header_value = serde_json::from_str::<Value>(&header).map_err(|error| {
        DomainError::InvalidData(format!("Failed to parse chat header JSON: {}", error))
    })?;
    extract_integrity_slug_from_header_value(&header_value)
}

async fn update_store_json_entry(dir: &Path, key: &str, value: Value) -> Result<(), DomainError> {
    fs::create_dir_all(dir).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create chat store directory {}: {}",
            dir.display(),
            error
        ))
    })?;

    let target = dir.join(format!("{}.json", key));
    let mut current = match fs::read(&target).await {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            DomainError::InvalidData(format!(
                "Chat store entry contains invalid JSON {}: {}",
                target.display(),
                error
            ))
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read chat store entry {}: {}",
                target.display(),
                error
            )));
        }
    };

    merge_json_value(&mut current, value);

    let temp = unique_temp_path(&target, "store.json");
    let bytes = serde_json::to_vec_pretty(&current).map_err(|error| {
        DomainError::InvalidData(format!("Failed to serialize chat store JSON: {}", error))
    })?;

    fs::write(&temp, &bytes).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write chat store temp file {}: {}",
            temp.display(),
            error
        ))
    })?;

    replace_file_with_fallback(&temp, &target).await?;
    Ok(())
}

async fn rename_store_json_entry(dir: &Path, key: &str, new_key: &str) -> Result<(), DomainError> {
    let from = dir.join(format!("{}.json", key));
    if !from.exists() {
        return Err(DomainError::NotFound(format!(
            "Chat store entry not found: {}",
            from.display()
        )));
    }

    let to = dir.join(format!("{}.json", new_key));
    if to.exists() {
        return Err(DomainError::InvalidData(format!(
            "Chat store entry already exists: {}",
            to.display()
        )));
    }

    fs::rename(&from, &to).await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to rename chat store entry {} to {}: {}",
            from.display(),
            to.display(),
            error
        ))
    })?;

    Ok(())
}

impl FileChatRepository {
    fn character_chat_store_root(&self, character_name: &str, integrity: &str) -> PathBuf {
        self.get_character_dir(character_name)
            .join(".tauritavern")
            .join(integrity)
    }

    fn group_chat_store_root(&self, chat_id: &str) -> PathBuf {
        self.group_chats_dir
            .join(".tauritavern")
            .join(Self::strip_jsonl_extension(chat_id))
    }

    async fn resolve_character_chat_store_dir(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
    ) -> Result<PathBuf, DomainError> {
        let namespace = validate_store_component(namespace, "namespace")?;
        let chat_path = self.get_chat_path(character_name, file_name);
        let integrity = read_chat_integrity_slug(&chat_path).await?;
        Ok(self
            .character_chat_store_root(character_name, &integrity)
            .join(namespace))
    }

    async fn resolve_group_chat_store_dir(
        &self,
        chat_id: &str,
        namespace: &str,
    ) -> Result<PathBuf, DomainError> {
        let namespace = validate_store_component(namespace, "namespace")?;
        Ok(self.group_chat_store_root(chat_id).join(namespace))
    }

    pub(super) async fn get_character_chat_store_json_value(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Value, DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_character_chat_store_dir(character_name, file_name, namespace)
            .await?;
        let path = dir.join(format!("{}.json", key));
        let bytes = fs::read(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Chat store entry not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to read chat store entry {}: {}",
                path.display(),
                error
            ))
        })?;

        serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            DomainError::InvalidData(format!(
                "Chat store entry contains invalid JSON {}: {}",
                path.display(),
                error
            ))
        })
    }

    pub(super) async fn get_group_chat_store_json_value(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<Value, DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_group_chat_store_dir(chat_id, namespace)
            .await?;
        let path = dir.join(format!("{}.json", key));
        let bytes = fs::read(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Chat store entry not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to read chat store entry {}: {}",
                path.display(),
                error
            ))
        })?;

        serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            DomainError::InvalidData(format!(
                "Chat store entry contains invalid JSON {}: {}",
                path.display(),
                error
            ))
        })
    }

    pub(super) async fn set_character_chat_store_json_value(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_character_chat_store_dir(character_name, file_name, namespace)
            .await?;
        fs::create_dir_all(&dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat store directory {}: {}",
                dir.display(),
                error
            ))
        })?;

        let target = dir.join(format!("{}.json", key));
        let temp = unique_temp_path(&target, "store.json");
        let bytes = serde_json::to_vec_pretty(&value).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize chat store JSON: {}", error))
        })?;

        fs::write(&temp, &bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write chat store temp file {}: {}",
                temp.display(),
                error
            ))
        })?;

        replace_file_with_fallback(&temp, &target).await?;
        Ok(())
    }

    pub(super) async fn update_character_chat_store_json_value(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_character_chat_store_dir(character_name, file_name, namespace)
            .await?;
        update_store_json_entry(&dir, &key, value).await
    }

    pub(super) async fn set_group_chat_store_json_value(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_group_chat_store_dir(chat_id, namespace)
            .await?;
        fs::create_dir_all(&dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create chat store directory {}: {}",
                dir.display(),
                error
            ))
        })?;

        let target = dir.join(format!("{}.json", key));
        let temp = unique_temp_path(&target, "store.json");
        let bytes = serde_json::to_vec_pretty(&value).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize chat store JSON: {}", error))
        })?;

        fs::write(&temp, &bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write chat store temp file {}: {}",
                temp.display(),
                error
            ))
        })?;

        replace_file_with_fallback(&temp, &target).await?;
        Ok(())
    }

    pub(super) async fn update_group_chat_store_json_value(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_group_chat_store_dir(chat_id, namespace)
            .await?;
        update_store_json_entry(&dir, &key, value).await
    }

    pub(super) async fn delete_character_chat_store_json_value(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_character_chat_store_dir(character_name, file_name, namespace)
            .await?;
        let path = dir.join(format!("{}.json", key));
        fs::remove_file(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Chat store entry not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to delete chat store entry {}: {}",
                path.display(),
                error
            ))
        })?;
        Ok(())
    }

    pub(super) async fn rename_character_chat_store_key_value(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let new_key = validate_store_component(new_key, "newKey")?;
        if key == new_key {
            return Ok(());
        }

        let dir = self
            .resolve_character_chat_store_dir(character_name, file_name, namespace)
            .await?;
        rename_store_json_entry(&dir, &key, &new_key).await
    }

    pub(super) async fn delete_group_chat_store_json_value(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let dir = self
            .resolve_group_chat_store_dir(chat_id, namespace)
            .await?;
        let path = dir.join(format!("{}.json", key));
        fs::remove_file(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Chat store entry not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to delete chat store entry {}: {}",
                path.display(),
                error
            ))
        })?;
        Ok(())
    }

    pub(super) async fn rename_group_chat_store_key_value(
        &self,
        chat_id: &str,
        namespace: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError> {
        let key = validate_store_component(key, "key")?;
        let new_key = validate_store_component(new_key, "newKey")?;
        if key == new_key {
            return Ok(());
        }

        let dir = self
            .resolve_group_chat_store_dir(chat_id, namespace)
            .await?;
        rename_store_json_entry(&dir, &key, &new_key).await
    }

    pub(super) async fn list_character_chat_store_keys_for_namespace(
        &self,
        character_name: &str,
        file_name: &str,
        namespace: &str,
    ) -> Result<Vec<String>, DomainError> {
        let dir = self
            .resolve_character_chat_store_dir(character_name, file_name, namespace)
            .await?;
        list_store_keys(&dir).await
    }

    pub(super) async fn list_group_chat_store_keys_for_namespace(
        &self,
        chat_id: &str,
        namespace: &str,
    ) -> Result<Vec<String>, DomainError> {
        let dir = self
            .resolve_group_chat_store_dir(chat_id, namespace)
            .await?;
        list_store_keys(&dir).await
    }
}

async fn list_store_keys(dir: &Path) -> Result<Vec<String>, DomainError> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read chat store directory {}: {}",
                dir.display(),
                error
            )));
        }
    };

    let mut keys = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read chat store directory entry {}: {}",
            dir.display(),
            error
        ))
    })? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        let stem = match path.file_stem().and_then(|value| value.to_str()) {
            Some(stem) => stem.trim(),
            None => continue,
        };
        if stem.is_empty() {
            continue;
        }

        keys.push(stem.to_string());
    }

    keys.sort();
    Ok(keys)
}
