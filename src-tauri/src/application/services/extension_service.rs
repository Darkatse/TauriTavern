use std::sync::Arc;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{
    Extension, ExtensionAssetPayload, ExtensionInstallResult, ExtensionUpdateResult,
    ExtensionVersion,
};
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::infrastructure::logging::logger;

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
        logger::debug("Getting all extensions");
        self.extension_repository.discover_extensions().await
    }

    /// Install an extension from a URL
    pub async fn install_extension(
        &self,
        url: &str,
        global: bool,
        branch: Option<String>,
    ) -> Result<ExtensionInstallResult, DomainError> {
        logger::debug(&format!("Installing extension from {}", url));
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
        logger::debug(&format!("Updating extension: {}", extension_name));
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
        logger::debug(&format!("Deleting extension: {}", extension_name));
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
        logger::debug(&format!("Getting extension version: {}", extension_name));
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
        logger::debug(&format!(
            "Moving extension: {} from {} to {}",
            extension_name, source, destination
        ));
        self.extension_repository
            .move_extension(extension_name, source, destination)
            .await
    }

    /// Read a file from a third-party extension directory.
    pub async fn read_third_party_asset(
        &self,
        extension_name: &str,
        relative_path: &str,
        location_hint: Option<&str>,
    ) -> Result<ExtensionAssetPayload, DomainError> {
        logger::debug(&format!(
            "Reading extension asset: {} / {}",
            extension_name, relative_path
        ));
        self.extension_repository
            .read_third_party_asset(extension_name, relative_path, location_hint)
            .await
    }
}
