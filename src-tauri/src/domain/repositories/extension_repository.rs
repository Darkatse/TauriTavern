use async_trait::async_trait;
use std::path::Path;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::{
    Extension, ExtensionInstallResult, ExtensionUpdateResult, ExtensionVersion,
};

#[async_trait]
pub trait ExtensionRepository: Send + Sync {
    /// Discover all available extensions
    async fn discover_extensions(&self) -> Result<Vec<Extension>, DomainError>;

    /// Get extension manifest
    async fn get_manifest(
        &self,
        extension_path: &Path,
    ) -> Result<Option<crate::domain::models::extension::ExtensionManifest>, DomainError>;

    /// Install an extension from a URL
    async fn install_extension(
        &self,
        url: &str,
        global: bool,
        branch: Option<String>,
    ) -> Result<ExtensionInstallResult, DomainError>;

    /// Update an extension
    async fn update_extension(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionUpdateResult, DomainError>;

    /// Delete an extension
    async fn delete_extension(&self, extension_name: &str, global: bool)
        -> Result<(), DomainError>;

    /// Get extension version information
    async fn get_extension_version(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<ExtensionVersion, DomainError>;

    /// Move an extension between local and global directories
    async fn move_extension(
        &self,
        extension_name: &str,
        source: &str,
        destination: &str,
    ) -> Result<(), DomainError>;
}
