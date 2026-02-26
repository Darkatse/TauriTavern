use async_trait::async_trait;
use mime_guess::from_path;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::background::{
    Background, BackgroundAsset, BackgroundImageMetadata, BackgroundImageMetadataIndex,
};
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::persistence::thumbnail_cache::{
    ThumbnailConfig, ThumbnailResizeMode, invalidate_thumbnail_cache, is_animated_image,
    read_thumbnail_or_original,
};

const THUMBNAIL_WIDTH: u32 = 160;
const THUMBNAIL_HEIGHT: u32 = 90;
const THUMBNAIL_QUALITY: u8 = 90;
const THUMBNAIL_RESOLUTION: u32 = THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT;

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

    /// Check if a file is an image
    fn is_image(&self, path: &Path) -> bool {
        if let Some(mime) = from_path(path).first() {
            return mime.type_() == "image";
        }
        false
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
        if self.backgrounds_dir.exists() {
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

    fn round_aspect_ratio(value: f64) -> f64 {
        (value * 10_000.0).round() / 10_000.0
    }

    fn system_time_to_timestamp_millis(time: SystemTime) -> Option<i64> {
        let millis = time.duration_since(UNIX_EPOCH).ok()?.as_millis();
        i64::try_from(millis).ok()
    }

    fn file_added_timestamp_millis(metadata: &std::fs::Metadata) -> i64 {
        metadata
            .created()
            .ok()
            .and_then(Self::system_time_to_timestamp_millis)
            .or_else(|| {
                metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_timestamp_millis)
            })
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
    }

    async fn build_background_metadata(
        &self,
        original_path: &Path,
    ) -> Result<Option<BackgroundImageMetadata>, DomainError> {
        let (width, height) = match image::image_dimensions(original_path) {
            Ok(dimensions) => dimensions,
            Err(error) => {
                logger::warn(&format!(
                    "Failed to read background dimensions '{}': {}",
                    original_path.display(),
                    error
                ));
                return Ok(None);
            }
        };

        if width == 0 || height == 0 {
            return Ok(None);
        }

        let file_metadata = fs::metadata(original_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read background metadata '{}': {}",
                original_path.display(),
                error
            ))
        })?;

        let is_animated = is_animated_image(original_path).await?;
        let dominant_color = if is_animated {
            Some("#808080".to_string())
        } else {
            None
        };

        Ok(Some(BackgroundImageMetadata {
            aspect_ratio: Self::round_aspect_ratio((width as f64) / (height as f64)),
            is_animated,
            dominant_color,
            added_timestamp: Self::file_added_timestamp_millis(&file_metadata),
            thumbnail_resolution: THUMBNAIL_RESOLUTION,
        }))
    }
}

#[async_trait]
impl BackgroundRepository for FileBackgroundRepository {
    async fn get_all_backgrounds(&self) -> Result<Vec<Background>, DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Getting all backgrounds from {:?}",
            self.backgrounds_dir
        ));

        self.ensure_backgrounds_dir_exists().await?;

        let mut entries = fs::read_dir(&self.backgrounds_dir).await.map_err(|error| {
            logger::error(&format!("Failed to read backgrounds directory: {}", error));
            DomainError::InternalError(format!("Failed to read backgrounds directory: {}", error))
        })?;

        let mut backgrounds = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            logger::error(&format!("Failed to read directory entry: {}", error));
            DomainError::InternalError(format!("Failed to read directory entry: {}", error))
        })? {
            let path = entry.path();
            if path.is_file() && self.is_image(&path) {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    backgrounds.push(Background::new(
                        filename.to_string(),
                        path.to_string_lossy().to_string(),
                    ));
                }
            }
        }

        Ok(backgrounds)
    }

    async fn get_all_background_metadata(
        &self,
    ) -> Result<BackgroundImageMetadataIndex, DomainError> {
        logger::debug("FileBackgroundRepository: Getting all background metadata");

        self.ensure_backgrounds_dir_exists().await?;

        let mut entries = fs::read_dir(&self.backgrounds_dir).await.map_err(|error| {
            logger::error(&format!(
                "Failed to read backgrounds directory for metadata: {}",
                error
            ));
            DomainError::InternalError(format!("Failed to read backgrounds directory: {}", error))
        })?;

        let mut images = HashMap::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            logger::error(&format!(
                "Failed to read background metadata directory entry: {}",
                error
            ));
            DomainError::InternalError(format!("Failed to read directory entry: {}", error))
        })? {
            let path = entry.path();
            if !path.is_file() || !self.is_image(&path) {
                continue;
            }

            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            if let Some(metadata) = self.build_background_metadata(&path).await? {
                images.insert(format!("backgrounds/{}", filename), metadata);
            }
        }

        Ok(BackgroundImageMetadataIndex { version: 1, images })
    }

    async fn delete_background(&self, filename: &str) -> Result<(), DomainError> {
        logger::debug(&format!(
            "FileBackgroundRepository: Deleting background: {}",
            filename
        ));

        let normalized = self.normalize_filename(filename)?;
        let file_path = self.backgrounds_dir.join(&normalized);
        if !file_path.exists() {
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

        if !old_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                old_filename
            )));
        }
        if new_path.exists() {
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

    async fn read_background_thumbnail(
        &self,
        filename: &str,
        _animated: bool,
    ) -> Result<BackgroundAsset, DomainError> {
        let normalized = self.normalize_filename(filename)?;
        let original_path = self.backgrounds_dir.join(&normalized);
        let thumbnail_path = self.thumbnail_cache_path(&normalized);
        let config = ThumbnailConfig {
            width: THUMBNAIL_WIDTH,
            height: THUMBNAIL_HEIGHT,
            quality: THUMBNAIL_QUALITY,
            resize_mode: ThumbnailResizeMode::PreserveArea,
        };

        let asset = read_thumbnail_or_original(&original_path, &thumbnail_path, config).await?;
        Ok(BackgroundAsset {
            bytes: asset.bytes,
            mime_type: asset.mime_type,
        })
    }
}

#[cfg(test)]
mod tests {
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
}
