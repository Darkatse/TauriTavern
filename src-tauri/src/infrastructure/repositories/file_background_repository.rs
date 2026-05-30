use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::background::BackgroundAsset;
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::thumbnail_cache::{
    invalidate_thumbnail_cache, read_thumbnail_or_original,
};
use crate::infrastructure::thumbnails::background_thumbnail_config;

/// File system implementation of the BackgroundRepository
pub struct FileBackgroundRepository {
    backgrounds_dir: PathBuf,
    thumbnails_bg_dir: PathBuf,
}

impl FileBackgroundRepository {
    /// Create a new FileBackgroundRepository instance
    pub fn new(backgrounds_dir: PathBuf, thumbnails_bg_dir: PathBuf) -> Self {
        Self {
            backgrounds_dir,
            thumbnails_bg_dir,
        }
    }

    fn normalize_filename(&self, filename: &str) -> Result<String, DomainError> {
        let sanitized = filename
            .chars()
            .map(|ch| match ch {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ if ch.is_control() => '_',
                _ => ch,
            })
            .collect::<String>();
        let sanitized = sanitized.trim().trim_end_matches(['.', ' ']).to_string();

        if sanitized.is_empty() {
            return Err(DomainError::InvalidData(
                "Invalid background filename".to_string(),
            ));
        }

        Ok(sanitized)
    }

    fn thumbnail_cache_path(&self, filename: &str) -> PathBuf {
        self.thumbnails_bg_dir.join(filename)
    }

    async fn ensure_backgrounds_dir_exists(&self) -> Result<(), DomainError> {
        if fs::try_exists(&self.backgrounds_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to check backgrounds directory '{}': {}",
                    self.backgrounds_dir.display(),
                    error
                ))
            })?
        {
            return Ok(());
        }

        fs::create_dir_all(&self.backgrounds_dir)
            .await
            .map_err(|error| {
                logger::error(&format!(
                    "Failed to create backgrounds directory: {}",
                    error
                ));
                DomainError::InternalError(format!(
                    "Failed to create backgrounds directory: {}",
                    error
                ))
            })
    }

    async fn invalidate_thumbnail_cache(&self, filename: &str) -> Result<(), DomainError> {
        let thumbnail_path = self.thumbnail_cache_path(filename);
        invalidate_thumbnail_cache(&thumbnail_path).await
    }
}

#[async_trait]
impl BackgroundRepository for FileBackgroundRepository {
    async fn delete_background(&self, filename: &str) -> Result<(), DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Deleting background: {}",
            filename
        ));

        let normalized = self.normalize_filename(filename)?;
        let file_path = self.backgrounds_dir.join(&normalized);
        if !fs::try_exists(&file_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to check background file '{}': {}",
                file_path.display(),
                error
            ))
        })? {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                filename
            )));
        }

        fs::remove_file(&file_path).await.map_err(|error| {
            logger::error(&format!("Failed to delete background file: {}", error));
            DomainError::InternalError(format!("Failed to delete background file: {}", error))
        })?;

        self.invalidate_thumbnail_cache(&normalized).await?;
        Ok(())
    }

    async fn rename_background(
        &self,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Renaming background from '{}' to '{}'",
            old_filename, new_filename
        ));

        let old_normalized = self.normalize_filename(old_filename)?;
        let new_normalized = self.normalize_filename(new_filename)?;
        let old_path = self.backgrounds_dir.join(&old_normalized);
        let new_path = self.backgrounds_dir.join(&new_normalized);

        if !fs::try_exists(&old_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to check background file '{}': {}",
                old_path.display(),
                error
            ))
        })? {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                old_filename
            )));
        }
        if fs::try_exists(&new_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to check background file '{}': {}",
                new_path.display(),
                error
            ))
        })? {
            return Err(DomainError::InvalidData(format!(
                "Background already exists: {}",
                new_filename
            )));
        }

        fs::rename(&old_path, &new_path).await.map_err(|error| {
            logger::error(&format!("Failed to rename background file: {}", error));
            DomainError::InternalError(format!("Failed to rename background file: {}", error))
        })?;

        self.invalidate_thumbnail_cache(&old_normalized).await?;
        self.invalidate_thumbnail_cache(&new_normalized).await?;
        Ok(())
    }

    async fn upload_background(&self, filename: &str, data: &[u8]) -> Result<String, DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Uploading background: {}",
            filename
        ));

        self.ensure_backgrounds_dir_exists().await?;

        let normalized = self.normalize_filename(filename)?;
        let file_path = self.backgrounds_dir.join(&normalized);
        fs::write(&file_path, data).await.map_err(|error| {
            logger::error(&format!("Failed to write background file: {}", error));
            DomainError::InternalError(format!("Failed to write background file: {}", error))
        })?;

        self.invalidate_thumbnail_cache(&normalized).await?;
        Ok(normalized)
    }

    async fn upload_background_from_path(
        &self,
        filename: &str,
        source_path: &Path,
    ) -> Result<String, DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Uploading background from path: {}",
            filename
        ));

        self.ensure_backgrounds_dir_exists().await?;

        let normalized = self.normalize_filename(filename)?;
        let file_path = self.backgrounds_dir.join(&normalized);

        fs::copy(source_path, &file_path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return DomainError::NotFound(format!(
                    "Source background file not found: {}",
                    source_path.display()
                ));
            }

            logger::error(&format!("Failed to copy background file: {}", error));
            DomainError::InternalError(format!("Failed to copy background file: {}", error))
        })?;

        self.invalidate_thumbnail_cache(&normalized).await?;
        Ok(normalized)
    }

    async fn read_background_thumbnail(
        &self,
        filename: &str,
        _animated: bool,
    ) -> Result<BackgroundAsset, DomainError> {
        let normalized = self.normalize_filename(filename)?;
        let original_path = self.backgrounds_dir.join(&normalized);
        let thumbnail_path = self.thumbnail_cache_path(&normalized);
        let asset = read_thumbnail_or_original(
            &original_path,
            &thumbnail_path,
            background_thumbnail_config(),
        )
        .await?;
        Ok(BackgroundAsset {
            bytes: asset.bytes,
            mime_type: asset.mime_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::repositories::background_repository::BackgroundRepository;
    use std::path::PathBuf;

    use super::FileBackgroundRepository;

    #[test]
    fn normalize_filename_replaces_unsafe_characters() {
        let repository = FileBackgroundRepository::new(
            PathBuf::from("backgrounds"),
            PathBuf::from("thumbnails/bg"),
        );
        let normalized = repository
            .normalize_filename("..\\bad:*name?.png")
            .expect("filename should be valid after normalization");

        assert_eq!(normalized, ".._bad__name_.png");
    }

    #[test]
    fn normalize_filename_rejects_empty_result() {
        let repository = FileBackgroundRepository::new(
            PathBuf::from("backgrounds"),
            PathBuf::from("thumbnails/bg"),
        );
        assert!(repository.normalize_filename(" ... ").is_err());
    }

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(test_name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("tauritavern-{test_name}-{}", uuid::Uuid::new_v4()));
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
    async fn upload_background_from_path_copies_file() {
        let temp = TempDirGuard::new("background-upload-from-path");
        let backgrounds_dir = temp.path.join("backgrounds");
        let thumbnails_dir = temp.path.join("thumbnails/bg");
        let source_path = temp.path.join("source.bin");

        tokio::fs::write(&source_path, b"ok")
            .await
            .expect("write source");

        let repository = FileBackgroundRepository::new(backgrounds_dir.clone(), thumbnails_dir);
        let uploaded = repository
            .upload_background_from_path("a.bin", &source_path)
            .await
            .expect("upload");

        assert_eq!(uploaded, "a.bin");
        let dest_bytes = tokio::fs::read(backgrounds_dir.join("a.bin"))
            .await
            .expect("read destination");
        assert_eq!(dest_bytes, b"ok");
    }
}
