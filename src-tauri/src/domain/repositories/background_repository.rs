use crate::domain::errors::DomainError;
use async_trait::async_trait;
use std::path::Path;

/// Repository interface for background images
#[async_trait]
pub trait BackgroundRepository: Send + Sync {
    /// Delete a background image by filename
    async fn delete_background(&self, filename: &str) -> Result<(), DomainError>;

    /// Rename a background image
    async fn rename_background(
        &self,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), DomainError>;

    /// Upload a new background image
    async fn upload_background(&self, filename: &str, data: &[u8]) -> Result<String, DomainError>;

    /// Upload a new background image from a local path.
    async fn upload_background_from_path(
        &self,
        filename: &str,
        source_path: &Path,
    ) -> Result<String, DomainError>;
}
