use std::sync::Arc;
use crate::domain::errors::DomainError;
use crate::domain::repositories::content_repository::ContentRepository;
use crate::infrastructure::logging::logger;

/// Content Service
pub struct ContentService {
    content_repository: Arc<dyn ContentRepository>,
}

impl ContentService {
    /// Create a new ContentService
    pub fn new(content_repository: Arc<dyn ContentRepository>) -> Self {
        Self {
            content_repository,
        }
    }

    /// Initialize default content
    pub async fn initialize_default_content(&self, user_handle: &str) -> Result<(), DomainError> {
        tracing::info!("Initializing default content");

        // Check if content is already initialized
        if self.is_default_content_initialized(user_handle).await? {
            tracing::info!("Default content already initialized");
            return Ok(());
        }

        // Copy default content to user directory
        self.content_repository.copy_default_content_to_user(user_handle).await?;

        tracing::info!("Default content initialized successfully");
        Ok(())
    }

    /// Check if default content is initialized
    pub async fn is_default_content_initialized(&self, user_handle: &str) -> Result<bool, DomainError> {
        logger::debug("Checking if default content is initialized");

        // Check if content is initialized
        let is_initialized = self.content_repository.is_default_content_initialized(user_handle).await?;

        logger::debug(&format!("Default content initialized: {}", is_initialized));

        Ok(is_initialized)
    }
}
