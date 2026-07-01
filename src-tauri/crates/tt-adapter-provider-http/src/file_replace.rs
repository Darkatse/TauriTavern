use std::io;
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;
use uuid::Uuid;

pub(crate) fn unique_temp_path(target_path: &Path, fallback_file_name: &str) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_file_name);

    target_path.with_file_name(format!("{}.{}.tmp", file_name, Uuid::new_v4()))
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

pub(crate) async fn replace_file_with_fallback(
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

                    match tokio::fs::remove_file(temp_path).await {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                        Err(error) => Err(DomainError::InternalError(format!(
                            "Replaced file {:?} -> {:?}, but failed to remove temp file: {}",
                            temp_path, target_path, error
                        ))),
                    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tt_domain::errors::DomainError;
    use uuid::Uuid;

    use super::{replace_file_with_fallback, unique_temp_path};

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tauritavern-provider-http-file-replace-{label}-{}",
            Uuid::new_v4()
        ))
    }

    #[test]
    fn unique_temp_path_is_unique_and_adjacent() {
        let root = temp_root("unique");
        let target = root.join("workflow.json");

        let a = unique_temp_path(&target, "fallback.json");
        let b = unique_temp_path(&target, "fallback.json");

        assert_ne!(a, b);
        assert_eq!(a.parent(), target.parent());
        assert_eq!(b.parent(), target.parent());

        let a_name = a.file_name().and_then(|value| value.to_str()).unwrap_or("");
        assert!(a_name.starts_with("workflow.json."));
        assert!(a_name.ends_with(".tmp"));
    }

    #[tokio::test]
    async fn replace_file_with_fallback_overwrites_existing_file() {
        let root = temp_root("overwrite");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let temp = root.join("workflow.tmp");
        let target = root.join("workflow.json");
        tokio::fs::write(&temp, b"new").await.unwrap();
        tokio::fs::write(&target, b"old").await.unwrap();

        replace_file_with_fallback(&temp, &target).await.unwrap();

        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"new");
        assert!(!temp.exists());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn replace_file_with_fallback_rejects_missing_temp() {
        let root = temp_root("missing");
        let error = replace_file_with_fallback(&root.join("missing.tmp"), &root.join("target.json"))
            .await
            .unwrap_err();

        assert!(matches!(error, DomainError::NotFound(_)));
    }

    #[tokio::test]
    async fn replace_file_with_fallback_copies_when_target_parent_is_missing() {
        let root = temp_root("missing-parent");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let temp = root.join("workflow.tmp");
        let target = root.join("nested").join("workflow.json");
        tokio::fs::write(&temp, b"workflow").await.unwrap();

        replace_file_with_fallback(&temp, &target).await.unwrap();

        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"workflow");
        assert!(!temp.exists());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn replace_file_with_fallback_rejects_directory_target_after_failed_rename() {
        let root = temp_root("directory-target");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let temp = root.join("workflow.tmp");
        let target = root.join("workflow.json");
        tokio::fs::write(&temp, b"workflow").await.unwrap();
        tokio::fs::create_dir_all(&target).await.unwrap();

        let error = replace_file_with_fallback(&temp, &target)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            DomainError::InvalidData(message) if message.contains("Target path is not a file")
        ));
        assert!(temp.exists());
        assert!(target.is_dir());
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
