use async_trait::async_trait;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use mime_guess::from_path;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::AsyncReadExt;

use crate::domain::errors::DomainError;
use crate::domain::models::background::{
    Background, BackgroundAsset, BackgroundImageMetadata, BackgroundImageMetadataIndex,
};
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::infrastructure::logging::logger;

const THUMBNAIL_WIDTH: u32 = 160;
const THUMBNAIL_HEIGHT: u32 = 90;
const THUMBNAIL_QUALITY: u8 = 90;
const THUMBNAIL_RESOLUTION: u32 = THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT;
const ANIMATED_EXTENSIONS: &[&str] = &[".mp4", ".webm", ".avi", ".mkv", ".flv", ".gif"];

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
        self.thumbnails_bg_dir.join(format!("{}.jpg", filename))
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

    async fn ensure_thumbnails_dir_exists(&self) -> Result<(), DomainError> {
        if self.thumbnails_bg_dir.exists() {
            return Ok(());
        }

        fs::create_dir_all(&self.thumbnails_bg_dir)
            .await
            .map_err(|error| {
                logger::error(&format!(
                    "Failed to create background thumbnails directory: {}",
                    error
                ));
                DomainError::InternalError(format!(
                    "Failed to create background thumbnails directory: {}",
                    error
                ))
            })
    }

    async fn invalidate_thumbnail_cache(&self, filename: &str) -> Result<(), DomainError> {
        let thumbnail_path = self.thumbnail_cache_path(filename);

        match fs::remove_file(&thumbnail_path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                logger::warn(&format!(
                    "Failed to remove thumbnail cache '{}': {}",
                    thumbnail_path.display(),
                    error
                ));
                Err(DomainError::InternalError(format!(
                    "Failed to remove thumbnail cache: {}",
                    error
                )))
            }
        }
    }

    async fn read_original_asset(
        &self,
        original_path: &Path,
    ) -> Result<BackgroundAsset, DomainError> {
        let bytes = fs::read(original_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read original background '{}': {}",
                original_path.display(),
                error
            ))
        })?;

        let mime_type = from_path(original_path)
            .first_or_octet_stream()
            .essence_str()
            .to_string();

        Ok(BackgroundAsset { bytes, mime_type })
    }

    fn extension_lowercase(path: &Path) -> String {
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!(".{}", extension.to_ascii_lowercase()))
            .unwrap_or_default()
    }

    fn is_apng_header(buffer: &[u8]) -> bool {
        buffer.windows(4).any(|chunk| chunk == b"acTL")
    }

    fn is_animated_webp_header(buffer: &[u8]) -> bool {
        buffer
            .windows(4)
            .any(|chunk| chunk == b"ANIM" || chunk == b"ANMF")
    }

    async fn should_serve_original(&self, original_path: &Path) -> Result<bool, DomainError> {
        let extension = Self::extension_lowercase(original_path);

        if ANIMATED_EXTENSIONS.contains(&extension.as_str()) {
            return Ok(true);
        }

        if extension == ".png" {
            let mut file = fs::File::open(original_path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect PNG header '{}': {}",
                    original_path.display(),
                    error
                ))
            })?;
            let mut header = vec![0u8; 512];
            let read_len = file.read(&mut header).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect PNG header '{}': {}",
                    original_path.display(),
                    error
                ))
            })?;
            header.truncate(read_len);

            if Self::is_apng_header(&header) {
                return Ok(true);
            }
        }

        if extension == ".webp" {
            let mut file = fs::File::open(original_path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect WebP header '{}': {}",
                    original_path.display(),
                    error
                ))
            })?;
            let mut header = vec![0u8; 512];
            let read_len = file.read(&mut header).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect WebP header '{}': {}",
                    original_path.display(),
                    error
                ))
            })?;
            header.truncate(read_len);

            if Self::is_animated_webp_header(&header) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn thumbnail_is_fresh(
        &self,
        thumbnail_path: &Path,
        original_path: &Path,
    ) -> Result<bool, DomainError> {
        let thumbnail_metadata = match fs::metadata(thumbnail_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read thumbnail metadata '{}': {}",
                    thumbnail_path.display(),
                    error
                )));
            }
        };

        let original_metadata = fs::metadata(original_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read background metadata '{}': {}",
                original_path.display(),
                error
            ))
        })?;

        let Some(original_modified) = original_metadata.modified().ok() else {
            return Ok(false);
        };
        let Some(thumbnail_modified) = thumbnail_metadata.modified().ok() else {
            return Ok(false);
        };

        Ok(original_modified <= thumbnail_modified)
    }

    async fn generate_thumbnail(
        &self,
        original_path: &Path,
        thumbnail_path: &Path,
    ) -> Result<(), DomainError> {
        let source_bytes = fs::read(original_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read background '{}': {}",
                original_path.display(),
                error
            ))
        })?;

        let source_image = image::load_from_memory(&source_bytes).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to decode background '{}': {}",
                original_path.display(),
                error
            ))
        })?;

        let source_width = source_image.width().max(1);
        let source_height = source_image.height().max(1);
        let aspect_ratio = source_width as f64 / source_height as f64;
        let target_area = (THUMBNAIL_WIDTH as f64) * (THUMBNAIL_HEIGHT as f64);

        let thumbnail_width = ((target_area * aspect_ratio).sqrt().round() as u32).max(1);
        let thumbnail_height = ((target_area / aspect_ratio).sqrt().round() as u32).max(1);

        let thumbnail_image =
            source_image.resize(thumbnail_width, thumbnail_height, FilterType::Triangle);

        let mut encoded = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut encoded, THUMBNAIL_QUALITY);
        encoder.encode_image(&thumbnail_image).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to encode thumbnail '{}': {}",
                original_path.display(),
                error
            ))
        })?;

        if let Some(parent) = thumbnail_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to ensure thumbnail directory '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        let temp_path = thumbnail_path.with_extension("tmp");
        fs::write(&temp_path, &encoded).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write temporary thumbnail '{}': {}",
                temp_path.display(),
                error
            ))
        })?;

        match fs::remove_file(thumbnail_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to replace thumbnail '{}': {}",
                    thumbnail_path.display(),
                    error
                )));
            }
        }

        fs::rename(&temp_path, thumbnail_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to finalize thumbnail '{}': {}",
                    thumbnail_path.display(),
                    error
                ))
            })
    }

    async fn ensure_thumbnail(
        &self,
        normalized_filename: &str,
        original_path: &Path,
    ) -> Result<PathBuf, DomainError> {
        self.ensure_thumbnails_dir_exists().await?;

        let thumbnail_path = self.thumbnail_cache_path(normalized_filename);
        if self
            .thumbnail_is_fresh(&thumbnail_path, original_path)
            .await?
        {
            return Ok(thumbnail_path);
        }

        self.generate_thumbnail(original_path, &thumbnail_path)
            .await?;
        Ok(thumbnail_path)
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
        filename: &str,
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

        let is_animated = self.should_serve_original(original_path).await?;
        let dominant_color = if is_animated {
            Some("#808080".to_string())
        } else {
            None
        };

        let aspect_ratio = Self::round_aspect_ratio((width as f64) / (height as f64));
        let added_timestamp = Self::file_added_timestamp_millis(&file_metadata);

        logger::debug(&format!(
            "Built background metadata for '{}': aspect_ratio={}",
            filename, aspect_ratio
        ));

        Ok(Some(BackgroundImageMetadata {
            aspect_ratio,
            is_animated,
            dominant_color,
            added_timestamp,
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

        // Read the directory
        let mut entries = fs::read_dir(&self.backgrounds_dir).await.map_err(|error| {
            logger::error(&format!("Failed to read backgrounds directory: {}", error));
            DomainError::InternalError(format!("Failed to read backgrounds directory: {}", error))
        })?;

        let mut backgrounds = Vec::new();

        // Process each entry
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            logger::error(&format!("Failed to read directory entry: {}", error));
            DomainError::InternalError(format!("Failed to read directory entry: {}", error))
        })? {
            let path = entry.path();

            // Check if it's an image file
            if path.is_file() && self.is_image(&path) {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    let background =
                        Background::new(filename.to_string(), path.to_string_lossy().to_string());
                    backgrounds.push(background);
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

            match self.build_background_metadata(filename, &path).await? {
                Some(metadata) => {
                    images.insert(format!("backgrounds/{}", filename), metadata);
                }
                None => {
                    logger::warn(&format!(
                        "Skipping background metadata for '{}'",
                        path.display()
                    ));
                }
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

        // Check if the file exists
        if !file_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                filename
            )));
        }

        // Delete the file
        fs::remove_file(&file_path).await.map_err(|e| {
            logger::error(&format!("Failed to delete background file: {}", e));
            DomainError::InternalError(format!("Failed to delete background file: {}", e))
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

        // Check if the source file exists
        if !old_path.exists() {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                old_filename
            )));
        }

        // Check if the destination file already exists
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

        // Write the file
        fs::write(&file_path, data).await.map_err(|e| {
            logger::error(&format!("Failed to write background file: {}", e));
            DomainError::InternalError(format!("Failed to write background file: {}", e))
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

        let original_metadata = fs::metadata(&original_path)
            .await
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => {
                    DomainError::NotFound(format!("Background not found: {}", filename))
                }
                _ => DomainError::InternalError(format!(
                    "Failed to read background metadata '{}': {}",
                    original_path.display(),
                    error
                )),
            })?;

        if !original_metadata.is_file() {
            return Err(DomainError::NotFound(format!(
                "Background not found: {}",
                filename
            )));
        }

        if self.should_serve_original(&original_path).await? {
            return self.read_original_asset(&original_path).await;
        }

        let thumbnail_path = match self.ensure_thumbnail(&normalized, &original_path).await {
            Ok(path) => path,
            Err(error) => {
                logger::warn(&format!(
                    "Failed to generate background thumbnail '{}': {}. Falling back to original.",
                    filename, error
                ));
                return self.read_original_asset(&original_path).await;
            }
        };

        let bytes = match fs::read(&thumbnail_path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                logger::warn(&format!(
                    "Failed to read background thumbnail '{}': {}. Falling back to original.",
                    thumbnail_path.display(),
                    error
                ));
                return self.read_original_asset(&original_path).await;
            }
        };

        Ok(BackgroundAsset {
            bytes,
            mime_type: "image/jpeg".to_string(),
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
