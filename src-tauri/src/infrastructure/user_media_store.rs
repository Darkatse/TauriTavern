use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use tokio::fs;

use crate::application::services::user_media_service::{
    UserMediaEntry, UserMediaStore, UserMediaStoreError,
};
use crate::domain::models::user_directory::UserDirectory;

#[derive(Debug, Clone)]
pub(crate) struct FilesystemUserMediaStore {
    user_images_dir: PathBuf,
}

impl FilesystemUserMediaStore {
    pub(crate) fn from_data_root(data_root: impl AsRef<Path>) -> Self {
        Self {
            user_images_dir: UserDirectory::default_user(data_root.as_ref()).user_images,
        }
    }

    fn resolve(&self, relative_path: &Path) -> PathBuf {
        self.user_images_dir.join(relative_path)
    }
}

#[async_trait]
impl UserMediaStore for FilesystemUserMediaStore {
    async fn write_file(
        &self,
        relative_path: &Path,
        bytes: Vec<u8>,
    ) -> Result<(), UserMediaStoreError> {
        let target = self.resolve(relative_path);
        let parent = target.parent().ok_or_else(|| {
            UserMediaStoreError::internal("Failed to resolve image upload directory")
        })?;

        fs::create_dir_all(parent).await.map_err(|error| {
            UserMediaStoreError::internal(format!("Failed to create image directory: {}", error))
        })?;

        fs::write(&target, bytes).await.map_err(|error| {
            UserMediaStoreError::internal(format!("Failed to save the image: {}", error))
        })
    }

    async fn ensure_folder(&self, relative_folder: &Path) -> Result<(), UserMediaStoreError> {
        fs::create_dir_all(self.resolve(relative_folder))
            .await
            .map_err(|error| {
                UserMediaStoreError::internal(format!("Unable to create directory: {}", error))
            })
    }

    async fn list_files(
        &self,
        relative_folder: &Path,
    ) -> Result<Vec<UserMediaEntry>, UserMediaStoreError> {
        let mut entries = fs::read_dir(self.resolve(relative_folder))
            .await
            .map_err(|error| {
                UserMediaStoreError::internal(format!("Unable to read images directory: {}", error))
            })?;

        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            UserMediaStoreError::internal(format!("Unable to read directory entry: {}", error))
        })? {
            let path = entry.path();
            let metadata = entry.metadata().await.map_err(|error| {
                UserMediaStoreError::internal(format!("Unable to stat media file: {}", error))
            })?;
            if !metadata.is_file() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let mime_type = mime_guess::from_path(&path)
                .first()
                .map(|mime| mime.essence_str().to_string());
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i128);

            files.push(UserMediaEntry {
                name,
                mime_type,
                modified_ms,
            });
        }

        Ok(files)
    }

    async fn list_folders(&self) -> Result<Vec<String>, UserMediaStoreError> {
        fs::create_dir_all(&self.user_images_dir)
            .await
            .map_err(|error| {
                UserMediaStoreError::internal(format!(
                    "Failed to ensure user images directory: {}",
                    error
                ))
            })?;

        let mut entries = fs::read_dir(&self.user_images_dir).await.map_err(|error| {
            UserMediaStoreError::internal(format!("Unable to read images directory: {}", error))
        })?;

        let mut folders = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            UserMediaStoreError::internal(format!("Unable to read directory entry: {}", error))
        })? {
            let metadata = entry.metadata().await.map_err(|error| {
                UserMediaStoreError::internal(format!("Unable to stat folder: {}", error))
            })?;

            if !metadata.is_dir() {
                continue;
            }

            let file_name = entry.file_name();
            let Some(name) = file_name.to_str().map(|value| value.to_string()) else {
                continue;
            };

            folders.push(name);
        }

        Ok(folders)
    }

    async fn delete_file(&self, relative_path: &Path) -> Result<(), UserMediaStoreError> {
        let target = self.resolve(relative_path);
        let metadata = fs::metadata(&target)
            .await
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => UserMediaStoreError::not_found("File not found"),
                _ => UserMediaStoreError::internal(format!("Failed to stat file: {}", error)),
            })?;

        if !metadata.is_file() {
            return Err(UserMediaStoreError::not_found("File not found"));
        }

        fs::remove_file(&target)
            .await
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => UserMediaStoreError::not_found("File not found"),
                _ => UserMediaStoreError::internal(format!("Failed to delete file: {}", error)),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::user_media_service::UserMediaStore;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(test_name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("tauritavern-{test_name}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test]
    async fn filesystem_store_writes_lists_and_deletes_media() {
        let temp = TempDirGuard::new("user-media-store");
        let store = FilesystemUserMediaStore::from_data_root(&temp.path);
        let relative = PathBuf::from("gallery").join("photo.png");

        store
            .write_file(&relative, b"ok".to_vec())
            .await
            .expect("write file");

        let listed = store
            .list_files(Path::new("gallery"))
            .await
            .expect("list files");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "photo.png");
        assert_eq!(listed[0].mime_type.as_deref(), Some("image/png"));
        assert!(listed[0].modified_ms.is_some());

        store.delete_file(&relative).await.expect("delete file");

        let listed = store
            .list_files(Path::new("gallery"))
            .await
            .expect("list files after delete");
        assert!(listed.is_empty());
    }
}
