use std::io;
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};
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
    tokio::fs::write(&tmp_path, contents)
        .await
        .map_err(|error| DomainError::InternalError(format!("Failed to write file: {error}")))?;
    replace_file_with_fallback(&tmp_path, path).await
}

fn json_tmp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("data.json");
    path.with_file_name(format!("{file_name}.{}.tmp", Uuid::new_v4()))
}

async fn replace_file_with_fallback(
    temp_path: &Path,
    target_path: &Path,
) -> Result<(), DomainError> {
    let Some(temp_metadata) = optional_metadata(temp_path).await? else {
        return Err(DomainError::NotFound(format!(
            "Temp file not found: {}",
            temp_path.display()
        )));
    };
    if !temp_metadata.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Temp path is not a file: {}",
            temp_path.display()
        )));
    }

    match tokio::fs::rename(temp_path, target_path).await {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let temp_after = optional_metadata(temp_path).await?;
            let target_after = optional_metadata(target_path).await?;

            match (temp_after, target_after) {
                (None, Some(target_metadata)) if target_metadata.is_file() => {
                    tracing::warn!(
                        "Rename reported an error after replacing file {:?} -> {:?}: {}",
                        temp_path,
                        target_path,
                        rename_error
                    );
                    Ok(())
                }
                (Some(temp_metadata), target_after) if temp_metadata.is_file() => {
                    if target_after.is_some_and(|metadata| !metadata.is_file()) {
                        return Err(DomainError::InvalidData(format!(
                            "Target path is not a file after failed replace: {}",
                            target_path.display()
                        )));
                    }

                    tracing::warn!(
                        "Rename failed while replacing file {:?} -> {:?}: {}. Falling back to copy/remove.",
                        temp_path,
                        target_path,
                        rename_error
                    );

                    if let Some(parent) = target_path.parent() {
                        tokio::fs::create_dir_all(parent).await.map_err(|error| {
                            DomainError::InternalError(format!(
                                "Failed to create target parent directory {:?}: {}",
                                parent, error
                            ))
                        })?;
                    }

                    tokio::fs::copy(temp_path, target_path)
                        .await
                        .map_err(|copy_error| {
                            DomainError::InternalError(format!(
                                "Failed to replace file {:?} -> {:?}. Rename error: {}. Copy fallback error: {}",
                                temp_path, target_path, rename_error, copy_error
                            ))
                        })?;
                    remove_temp_file(temp_path, target_path).await
                }
                (Some(_), _) => Err(DomainError::InvalidData(format!(
                    "Temp path is not a file after failed replace: {}",
                    temp_path.display()
                ))),
                (None, None) => Err(DomainError::InternalError(format!(
                    "Failed to replace file {:?} -> {:?}. Rename error: {}. Temp and target are both missing after failure.",
                    temp_path, target_path, rename_error
                ))),
                (None, Some(_)) => Err(DomainError::InvalidData(format!(
                    "Target path is not a file after failed replace: {}",
                    target_path.display()
                ))),
            }
        }
    }
}

async fn optional_metadata(path: &Path) -> Result<Option<std::fs::Metadata>, DomainError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to read file metadata {:?}: {}",
            path, error
        ))),
    }
}

async fn remove_temp_file(temp_path: &Path, target_path: &Path) -> Result<(), DomainError> {
    match tokio::fs::remove_file(temp_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::InternalError(format!(
            "Replaced file {:?} -> {:?}, but failed to remove temp file: {}",
            temp_path, target_path, error
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use serde_json::{Value, json};
    use tt_domain::errors::DomainError;
    use uuid::Uuid;

    use super::{read_json_file, replace_file_with_fallback, write_json_file};

    fn temp_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tauritavern-sync-json-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn read_json_file_reports_missing_file_as_not_found() {
        let path = temp_root().join("missing.json");

        let error = read_json_file::<Value>(&path).await.unwrap_err();

        assert!(matches!(error, DomainError::NotFound(_)));
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
    async fn write_json_file_creates_parent_directory_and_round_trips() {
        let root = temp_root();
        let path = root.join("a").join("b").join("settings.json");
        let expected = json!({ "version": 1, "name": "demo" });

        write_json_file(&path, &expected).await.expect("write json");

        let actual: Value = read_json_file(&path).await.expect("read json");
        assert_eq!(actual, expected);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn write_json_file_replaces_target_entry() {
        let root = temp_root();
        let path = root.join("settings.json");
        write_json_file(&path, &json!({ "version": 1, "payload": "old" }))
            .await
            .expect("write initial json");
        let mut old_handle = std::fs::File::open(&path).expect("open old handle");

        write_json_file(&path, &json!({ "version": 2, "payload": "new" }))
            .await
            .expect("write updated json");

        let mut old_contents = String::new();
        old_handle
            .read_to_string(&mut old_contents)
            .expect("read old handle");
        let on_disk_contents = tokio::fs::read_to_string(&path).await.expect("read path");

        let old_json: Value = serde_json::from_str(&old_contents).expect("parse old json");
        let new_json: Value = serde_json::from_str(&on_disk_contents).expect("parse new json");
        assert_eq!(old_json.get("version"), Some(&json!(1)));
        assert_eq!(new_json.get("version"), Some(&json!(2)));
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
    async fn replace_file_with_fallback_copies_when_target_parent_is_missing() {
        let root = temp_root();
        let temp = root.join("temp.json");
        let target = root.join("nested").join("state.json");
        tokio::fs::create_dir_all(&root)
            .await
            .expect("create temp root");
        tokio::fs::write(&temp, br#"{"ok":true}"#)
            .await
            .expect("write temp");

        replace_file_with_fallback(&temp, &target)
            .await
            .expect("replace through fallback");

        let contents = tokio::fs::read_to_string(&target)
            .await
            .expect("read target");
        assert_eq!(contents, r#"{"ok":true}"#);
        assert!(!temp.exists());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn write_json_file_rejects_directory_target_after_failed_replace() {
        let root = temp_root();
        let target = root.join("state.json");
        tokio::fs::create_dir_all(&target)
            .await
            .expect("create directory target");

        let error = write_json_file(&target, &json!({ "ok": true }))
            .await
            .unwrap_err();

        assert!(matches!(error, DomainError::InvalidData(_)));
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
