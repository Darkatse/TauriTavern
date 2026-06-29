use crate::domain::errors::DomainError;
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::domain::repositories::image_metadata_repository::ImageMetadataRepository;
use std::path::Path;
use std::sync::Arc;

/// Service for managing background images
pub struct BackgroundService {
    repository: Arc<dyn BackgroundRepository>,
    image_metadata_repository: Arc<dyn ImageMetadataRepository>,
}

impl BackgroundService {
    /// Create a new BackgroundService instance
    pub fn new(
        repository: Arc<dyn BackgroundRepository>,
        image_metadata_repository: Arc<dyn ImageMetadataRepository>,
    ) -> Self {
        Self {
            repository,
            image_metadata_repository,
        }
    }

    /// Delete a background image by filename
    pub async fn delete_background(&self, filename: &str) -> Result<(), DomainError> {
        tracing::debug!("BackgroundService: Deleting background: {}", filename);

        // Validate filename
        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "Background filename cannot be empty".to_string(),
            ));
        }

        self.repository.delete_background(filename).await?;
        self.image_metadata_repository
            .remove_background_metadata(filename)
            .await
    }

    /// Rename a background image
    pub async fn rename_background(
        &self,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), DomainError> {
        tracing::debug!(
            "BackgroundService: Renaming background from '{}' to '{}'",
            old_filename,
            new_filename
        );

        // Validate filenames
        if old_filename.is_empty() || new_filename.is_empty() {
            return Err(DomainError::InvalidData(
                "Background filenames cannot be empty".to_string(),
            ));
        }

        if old_filename == new_filename {
            return Err(DomainError::InvalidData(
                "New filename must be different from old filename".to_string(),
            ));
        }

        self.repository
            .rename_background(old_filename, new_filename)
            .await?;
        self.image_metadata_repository
            .rename_background_metadata(old_filename, new_filename)
            .await
    }

    /// Upload a new background image
    pub async fn upload_background(
        &self,
        filename: &str,
        data: &[u8],
    ) -> Result<String, DomainError> {
        tracing::debug!("BackgroundService: Uploading background: {}", filename);

        // Validate filename and data
        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "Background filename cannot be empty".to_string(),
            ));
        }

        if data.is_empty() {
            return Err(DomainError::InvalidData(
                "Background data cannot be empty".to_string(),
            ));
        }

        self.repository.upload_background(filename, data).await
    }

    pub async fn upload_background_from_path(
        &self,
        filename: &str,
        source_path: impl AsRef<Path>,
    ) -> Result<String, DomainError> {
        tracing::debug!(
            "BackgroundService: Uploading background from path: {}",
            filename
        );

        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "Background filename cannot be empty".to_string(),
            ));
        }

        let source_path = source_path.as_ref();
        if source_path.as_os_str().is_empty() {
            return Err(DomainError::InvalidData(
                "Background source path cannot be empty".to_string(),
            ));
        }

        self.repository
            .upload_background_from_path(filename, source_path)
            .await
    }
}
