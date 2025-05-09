use async_trait::async_trait;
use crate::domain::errors::DomainError;
use crate::domain::models::background::Background;

/// Repository interface for background images
#[async_trait]
pub trait BackgroundRepository: Send + Sync {
    /// Get all background images
    async fn get_all_backgrounds(&self) -> Result<Vec<Background>, DomainError>;
    
    /// Delete a background image by filename
    async fn delete_background(&self, filename: &str) -> Result<(), DomainError>;
    
    /// Rename a background image
    async fn rename_background(&self, old_filename: &str, new_filename: &str) -> Result<(), DomainError>;
    
    /// Upload a new background image
    async fn upload_background(&self, filename: &str, data: &[u8]) -> Result<String, DomainError>;
}
