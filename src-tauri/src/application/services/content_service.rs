use crate::domain::errors::DomainError;
use crate::domain::repositories::content_repository::ContentRepository;
use std::sync::Arc;

/// Content Service
pub struct ContentService {
    content_repository: Arc<dyn ContentRepository>,
}

impl ContentService {
    /// Create a new ContentService
    pub fn new(content_repository: Arc<dyn ContentRepository>) -> Self {
        Self { content_repository }
    }

    /// Initialize default content
    pub async fn initialize_default_content(&self, user_handle: &str) -> Result<(), DomainError> {
        tracing::debug!("Synchronizing default content");

        self.content_repository
            .copy_default_content_to_user(user_handle)
            .await?;

        tracing::debug!("Default content synchronized successfully");
        Ok(())
    }

    /// Check if default content is initialized
    pub async fn is_default_content_initialized(
        &self,
        user_handle: &str,
    ) -> Result<bool, DomainError> {
        tracing::debug!("Checking if default content is initialized");

        // Check if content is initialized
        let is_initialized = self
            .content_repository
            .is_default_content_initialized(user_handle)
            .await?;

        tracing::debug!("Default content initialized: {}", is_initialized);

        Ok(is_initialized)
    }
}
