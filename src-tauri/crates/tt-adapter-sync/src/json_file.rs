use std::io;
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::AsyncWriteExt;
use tt_domain::errors::DomainError;
use uuid::Uuid;

pub(crate) async fn read_json_file<T>(path: &Path) -> Result<T, DomainError>
where
    T: DeserializeOwned,
{
    let contents = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => {
                DomainError::NotFound(format!("File not found: {}", path.display()))
            }
            _ => DomainError::InternalError(format!("Failed to read file: {error}")),
        })?;
    serde_json::from_str(&contents)
        .map_err(|error| DomainError::InvalidData(format!("Invalid JSON: {error}")))
}

pub(crate) async fn write_json_file<T>(path: &Path, value: &T) -> Result<(), DomainError>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create parent directory {:?}: {}",
                parent, error
            ))
        })?;
    }

    let contents = serde_json::to_vec_pretty(value).map_err(|error| {
        DomainError::InvalidData(format!("Failed to serialize to JSON: {error}"))
    })?;
    let tmp_path = json_tmp_path(path);
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to create {}: {error}", tmp_path.display()))
        })?;
    let result = async {
        file.write_all(&contents).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&tmp_path, path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
    result.map_err(|error| {
        DomainError::InternalError(format!("Failed to save {}: {error}", path.display()))
    })
}

fn json_tmp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("data.json");
    path.with_file_name(format!("{file_name}.{}.tmp", Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tt_domain::errors::DomainError;
    use uuid::Uuid;

    use super::{read_json_file, write_json_file};

    fn temp_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tauritavern-sync-json-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn read_json_file_reports_malformed_json_as_invalid_data() {
        let root = temp_root();
        let path = root.join("bad.json");
        tokio::fs::create_dir_all(&root)
            .await
            .expect("create temp root");
        tokio::fs::write(&path, b"{")
            .await
            .expect("write malformed json");

        let error = read_json_file::<Value>(&path).await.unwrap_err();

        assert!(matches!(error, DomainError::InvalidData(_)));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn write_json_file_does_not_modify_target_on_serialization_error() {
        struct FailingSerialize;

        impl serde::Serialize for FailingSerialize {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("intentional serialization error"))
            }
        }

        let root = temp_root();
        let path = root.join("settings.json");
        write_json_file(&path, &json!({ "ok": true }))
            .await
            .expect("write initial json");
        let before: Value = read_json_file(&path).await.expect("read before");

        let error = write_json_file(&path, &FailingSerialize).await.unwrap_err();

        assert!(matches!(error, DomainError::InvalidData(_)));
        let after: Value = read_json_file(&path).await.expect("read after");
        assert_eq!(after, before);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn write_json_file_keeps_directory_target_on_failure() {
        let root = temp_root();
        let target = root.join("state.json");
        tokio::fs::create_dir_all(&target)
            .await
            .expect("create directory target");

        write_json_file(&target, &json!({ "ok": true }))
            .await
            .unwrap_err();

        assert!(target.is_dir());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
