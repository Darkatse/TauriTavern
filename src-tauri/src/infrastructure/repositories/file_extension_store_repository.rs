use async_trait::async_trait;
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::json_merge::merge_json_value;
use crate::domain::repositories::extension_store_repository::ExtensionStoreRepository;
use crate::infrastructure::persistence::file_system::{
    replace_file_with_fallback, unique_temp_path,
};

pub struct FileExtensionStoreRepository {
    base_dir: PathBuf,
}

impl FileExtensionStoreRepository {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn namespace_root(&self, namespace: &str) -> Result<PathBuf, DomainError> {
        let namespace = validate_component(namespace, "namespace")?;
        Ok(self.base_dir.join(namespace))
    }

    fn kv_table_dir(&self, namespace: &str, table: &str) -> Result<PathBuf, DomainError> {
        let table = validate_component(table, "table")?;
        Ok(self.namespace_root(namespace)?.join("kv").join(table))
    }

    fn blob_table_dir(&self, namespace: &str, table: &str) -> Result<PathBuf, DomainError> {
        let table = validate_component(table, "table")?;
        Ok(self.namespace_root(namespace)?.join("blobs").join(table))
    }

    fn json_entry_path(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
    ) -> Result<PathBuf, DomainError> {
        let key = validate_component(key, "key")?;
        Ok(self
            .kv_table_dir(namespace, table)?
            .join(format!("{}.json", key)))
    }

    fn blob_entry_path(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
    ) -> Result<PathBuf, DomainError> {
        let key = validate_component(key, "key")?;
        Ok(self.blob_table_dir(namespace, table)?.join(key))
    }
}

fn validate_component(raw: &str, label: &str) -> Result<String, DomainError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(format!(
            "Extension store {} cannot be empty",
            label
        )));
    }

    if matches!(value, "." | "..") {
        return Err(DomainError::InvalidData(format!(
            "Extension store {} cannot be '.' or '..'",
            label
        )));
    }

    if value.starts_with('.') {
        return Err(DomainError::InvalidData(format!(
            "Extension store {} cannot start with '.'",
            label
        )));
    }

    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(DomainError::InvalidData(format!(
            "Extension store {} contains illegal characters",
            label
        )));
    }

    Ok(value.to_string())
}

async fn list_json_keys_in_dir(dir: &Path) -> Result<Vec<String>, DomainError> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read extension store directory {}: {}",
                dir.display(),
                error
            )));
        }
    };

    let mut keys = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read extension store directory entry {}: {}",
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

async fn list_file_names_in_dir(dir: &Path) -> Result<Vec<String>, DomainError> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read extension store directory {}: {}",
                dir.display(),
                error
            )));
        }
    };

    let mut keys = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read extension store directory entry {}: {}",
            dir.display(),
            error
        ))
    })? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let name = match path.file_name().and_then(|value| value.to_str()) {
            Some(name) => name.trim(),
            None => continue,
        };
        if name.is_empty() {
            continue;
        }

        keys.push(name.to_string());
    }

    keys.sort();
    Ok(keys)
}

async fn list_directories(dir: &Path) -> Result<Vec<String>, DomainError> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to read extension store directory {}: {}",
                dir.display(),
                error
            )));
        }
    };

    let mut dirs = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read extension store directory entry {}: {}",
            dir.display(),
            error
        ))
    })? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|value| value.to_str()) {
            Some(name) => name.trim(),
            None => continue,
        };
        if name.is_empty() {
            continue;
        }

        dirs.push(name.to_string());
    }

    dirs.sort();
    Ok(dirs)
}

#[async_trait]
impl ExtensionStoreRepository for FileExtensionStoreRepository {
    async fn get_json(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
    ) -> Result<Value, DomainError> {
        let path = self.json_entry_path(namespace, table, key)?;
        let bytes = fs::read(&path).await.map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Extension store JSON entry not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to read extension store JSON entry {}: {}",
                path.display(),
                error
            ))
        })?;

        serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            DomainError::InvalidData(format!(
                "Extension store entry contains invalid JSON {}: {}",
                path.display(),
                error
            ))
        })
    }

    async fn set_json(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let path = self.json_entry_path(namespace, table, key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create extension store directory {}: {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        let bytes = serde_json::to_vec_pretty(&value).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to serialize extension store JSON: {}",
                error
            ))
        })?;

        let temp = unique_temp_path(&path, "extension-store.json");
        fs::write(&temp, &bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write extension store temp file {}: {}",
                temp.display(),
                error
            ))
        })?;

        replace_file_with_fallback(&temp, &path).await?;
        Ok(())
    }

    async fn update_json(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        let path = self.json_entry_path(namespace, table, key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create extension store directory {}: {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        let mut current = match fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Extension store entry contains invalid JSON {}: {}",
                    path.display(),
                    error
                ))
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Value::Null,
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read extension store JSON entry {}: {}",
                    path.display(),
                    error
                )));
            }
        };

        merge_json_value(&mut current, value);

        let bytes = serde_json::to_vec_pretty(&current).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to serialize extension store JSON: {}",
                error
            ))
        })?;

        let temp = unique_temp_path(&path, "extension-store.json");
        fs::write(&temp, &bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write extension store temp file {}: {}",
                temp.display(),
                error
            ))
        })?;

        replace_file_with_fallback(&temp, &path).await?;
        Ok(())
    }

    async fn rename_json_key(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError> {
        let from = self.json_entry_path(namespace, table, key)?;
        let to = self.json_entry_path(namespace, table, new_key)?;
        if from == to {
            return Ok(());
        }

        if !from.exists() {
            return Err(DomainError::NotFound(format!(
                "Extension store JSON entry not found: {}",
                from.display()
            )));
        }

        if to.exists() {
            return Err(DomainError::InvalidData(format!(
                "Extension store JSON entry already exists: {}",
                to.display()
            )));
        }

        fs::rename(&from, &to).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to rename extension store JSON entry {} to {}: {}",
                from.display(),
                to.display(),
                error
            ))
        })?;

        Ok(())
    }

    async fn delete_json(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
    ) -> Result<(), DomainError> {
        let path = self.json_entry_path(namespace, table, key)?;
        fs::remove_file(&path).await.map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Extension store JSON entry not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to delete extension store JSON entry {}: {}",
                path.display(),
                error
            ))
        })?;
        Ok(())
    }

    async fn list_json_keys(
        &self,
        namespace: &str,
        table: &str,
    ) -> Result<Vec<String>, DomainError> {
        let dir = self.kv_table_dir(namespace, table)?;
        list_json_keys_in_dir(&dir).await
    }

    async fn list_tables(&self, namespace: &str) -> Result<Vec<String>, DomainError> {
        let root = self.namespace_root(namespace)?;
        let kv_tables = list_directories(&root.join("kv")).await?;
        let blob_tables = list_directories(&root.join("blobs")).await?;

        let mut merged = kv_tables;
        for table in blob_tables {
            if !merged.contains(&table) {
                merged.push(table);
            }
        }

        merged.sort();
        Ok(merged)
    }

    async fn delete_table(&self, namespace: &str, table: &str) -> Result<(), DomainError> {
        let kv_dir = self.kv_table_dir(namespace, table)?;
        let blob_dir = self.blob_table_dir(namespace, table)?;

        let mut removed_any = false;

        match fs::remove_dir_all(&kv_dir).await {
            Ok(_) => removed_any = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to delete extension store table {}: {}",
                    kv_dir.display(),
                    error
                )));
            }
        }

        match fs::remove_dir_all(&blob_dir).await {
            Ok(_) => removed_any = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to delete extension store table {}: {}",
                    blob_dir.display(),
                    error
                )));
            }
        }

        if !removed_any {
            return Err(DomainError::NotFound(format!(
                "Extension store table not found: {}:{}",
                namespace, table
            )));
        }

        Ok(())
    }

    async fn get_blob(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
    ) -> Result<Vec<u8>, DomainError> {
        let path = self.blob_entry_path(namespace, table, key)?;
        fs::read(&path).await.map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Extension store blob not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to read extension store blob {}: {}",
                path.display(),
                error
            ))
        })
    }

    async fn set_blob(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        bytes: Vec<u8>,
    ) -> Result<(), DomainError> {
        let path = self.blob_entry_path(namespace, table, key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create extension store directory {}: {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        let temp = unique_temp_path(&path, "extension-store.blob");
        fs::write(&temp, &bytes).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write extension store temp file {}: {}",
                temp.display(),
                error
            ))
        })?;

        replace_file_with_fallback(&temp, &path).await?;
        Ok(())
    }

    async fn delete_blob(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
    ) -> Result<(), DomainError> {
        let path = self.blob_entry_path(namespace, table, key)?;
        fs::remove_file(&path).await.map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Extension store blob not found: {}",
                    path.display()
                ));
            }
            DomainError::InternalError(format!(
                "Failed to delete extension store blob {}: {}",
                path.display(),
                error
            ))
        })?;
        Ok(())
    }

    async fn list_blob_keys(
        &self,
        namespace: &str,
        table: &str,
    ) -> Result<Vec<String>, DomainError> {
        let dir = self.blob_table_dir(namespace, table)?;
        list_file_names_in_dir(&dir).await
    }
}

#[cfg(test)]
mod tests {
    use super::FileExtensionStoreRepository;
    use crate::domain::repositories::extension_store_repository::ExtensionStoreRepository;
    use serde_json::json;
    use std::path::PathBuf;

    fn create_temp_dir() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-extension-store-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    #[tokio::test]
    async fn json_round_trip_and_update_merges_objects() {
        let dir = create_temp_dir();
        let repo = FileExtensionStoreRepository::new(dir);

        repo.set_json(
            "my-ext",
            "main",
            "index",
            json!({"a": 1, "nested": {"x": 1}}),
        )
        .await
        .unwrap();
        repo.update_json(
            "my-ext",
            "main",
            "index",
            json!({"b": 2, "nested": {"y": 2}}),
        )
        .await
        .unwrap();

        let value = repo.get_json("my-ext", "main", "index").await.unwrap();
        assert_eq!(value, json!({"a": 1, "b": 2, "nested": {"x": 1, "y": 2}}));

        let keys = repo.list_json_keys("my-ext", "main").await.unwrap();
        assert_eq!(keys, vec![String::from("index")]);
    }

    #[tokio::test]
    async fn rename_and_delete_json_key() {
        let dir = create_temp_dir();
        let repo = FileExtensionStoreRepository::new(dir);

        repo.set_json("my-ext", "main", "k1", json!({"ok": true}))
            .await
            .unwrap();
        repo.rename_json_key("my-ext", "main", "k1", "k2")
            .await
            .unwrap();

        let keys = repo.list_json_keys("my-ext", "main").await.unwrap();
        assert_eq!(keys, vec![String::from("k2")]);

        repo.delete_json("my-ext", "main", "k2").await.unwrap();
        let keys = repo.list_json_keys("my-ext", "main").await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn list_and_delete_tables() {
        let dir = create_temp_dir();
        let repo = FileExtensionStoreRepository::new(dir);

        repo.set_json("my-ext", "main", "k1", json!(1))
            .await
            .unwrap();
        repo.set_json("my-ext", "extra", "k2", json!(2))
            .await
            .unwrap();

        let tables = repo.list_tables("my-ext").await.unwrap();
        assert_eq!(tables, vec![String::from("extra"), String::from("main")]);

        repo.delete_table("my-ext", "extra").await.unwrap();
        let tables = repo.list_tables("my-ext").await.unwrap();
        assert_eq!(tables, vec![String::from("main")]);
    }

    #[tokio::test]
    async fn blob_round_trip() {
        let dir = create_temp_dir();
        let repo = FileExtensionStoreRepository::new(dir);

        repo.set_blob("my-ext", "main", "icon.png", vec![1, 2, 3, 4])
            .await
            .unwrap();

        let keys = repo.list_blob_keys("my-ext", "main").await.unwrap();
        assert_eq!(keys, vec![String::from("icon.png")]);

        let bytes = repo.get_blob("my-ext", "main", "icon.png").await.unwrap();
        assert_eq!(bytes, vec![1, 2, 3, 4]);

        repo.delete_blob("my-ext", "main", "icon.png")
            .await
            .unwrap();
        let keys = repo.list_blob_keys("my-ext", "main").await.unwrap();
        assert!(keys.is_empty());
    }
}
