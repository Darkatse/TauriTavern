use std::sync::Arc;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{
    Extension, ExtensionInstallResult, ExtensionUpdateResult, ExtensionVersion,
};
use crate::domain::repositories::extension_repository::ExtensionRepository;

/// Extension service
pub struct ExtensionService {
    extension_repository: Arc<dyn ExtensionRepository>,
}

impl ExtensionService {
    /// Create a new extension service
    pub fn new(extension_repository: Arc<dyn ExtensionRepository>) -> Self {
        Self {
            extension_repository,
        }
    }

    /// Get all extensions
    pub async fn get_extensions(&self) -> Result<Vec<Extension>, DomainError> {
        tracing::debug!("Getting all extensions");
        self.extension_repository.discover_extensions().await
    }

    /// Install an extension from a URL
    pub async fn install_extension(
        &self,
        url: &str,
        global: bool,
        branch: Option<String>,
    ) -> Result<ExtensionInstallResult, DomainError> {
        tracing::debug!("Installing extension from {}", url);
        self.extension_repository
            .install_extension(url, global, branch)
            .await
    }

    /// Update an extension
    pub async fn update_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionUpdateResult, DomainError> {
        tracing::debug!("Updating extension: {}", extension_name);
        self.extension_repository
            .update_extension(extension_name, global)
            .await
    }

    /// Delete an extension
    pub async fn delete_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<(), DomainError> {
        tracing::debug!("Deleting extension: {}", extension_name);
        self.extension_repository
            .delete_extension(extension_name, global)
            .await
    }

    /// Get extension version information
    pub async fn get_extension_version(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionVersion, DomainError> {
        tracing::debug!("Getting extension version: {}", extension_name);
        self.extension_repository
            .get_extension_version(extension_name, global)
            .await
    }

    /// Move an extension between local and global directories
    pub async fn move_extension(
        &self,
        extension_name: &str,
        source: &str,
        destination: &str,
    ) -> Result<(), DomainError> {
        tracing::debug!(
            "Moving extension: {} from {} to {}",
            extension_name,
            source,
            destination
        );
        self.extension_repository
            .move_extension(extension_name, source, destination)
            .await
    }
}
