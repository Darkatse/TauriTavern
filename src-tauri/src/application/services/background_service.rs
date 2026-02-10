use crate::domain::errors::DomainError;
use crate::domain::models::background::Background;
use crate::domain::repositories::background_repository::BackgroundRepository;
use crate::infrastructure::logging::logger;
use std::sync::Arc;

/// Service for managing background images
pub struct BackgroundService {
    repository: Arc<dyn BackgroundRepository>,
}

impl BackgroundService {
    /// Create a new BackgroundService instance
    pub fn new(repository: Arc<dyn BackgroundRepository>) -> Self {
        Self { repository }
    }

    /// Get all background images
    pub async fn get_all_backgrounds(&self) -> Result<Vec<Background>, DomainError> {
        logger::debug("BackgroundService: Getting all backgrounds");
        self.repository.get_all_backgrounds().await
    }

    /// Delete a background image by filename
    pub async fn delete_background(&self, filename: &str) -> Result<(), DomainError> {
        logger::debug(&format!(
            "BackgroundService: Deleting background: {}",
            filename
        ));

        // Validate filename
        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "Background filename cannot be empty".to_string(),
            ));
        }

        self.repository.delete_background(filename).await
    }

    /// Rename a background image
    pub async fn rename_background(
        &self,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), DomainError> {
        logger::debug(&format!(
            "BackgroundService: Renaming background from '{}' to '{}'",
            old_filename, new_filename
        ));

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
            .await
    }

    /// Upload a new background image
    pub async fn upload_background(
        &self,
        filename: &str,
        data: &[u8],
    ) -> Result<String, DomainError> {
        logger::debug(&format!(
            "BackgroundService: Uploading background: {}",
            filename
        ));

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
}
