use async_trait::async_trait;

use tt_domain::errors::DomainError;
use tt_domain::models::extension::{
    Extension, ExtensionBranch, ExtensionInstallResult, ExtensionUpdateResult, ExtensionVersion,
};

#[async_trait]
pub trait ExtensionRepository: Send + Sync {
    /// Discover all available extensions
    async fn discover_extensions(&self) -> Result<Vec<Extension>, DomainError>;

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

    /// List remote branches available to an extension.
    async fn get_extension_branches(
        &self,
        extension_name: &str,
        global: bool,
    ) -> Result<Vec<ExtensionBranch>, DomainError>;

    /// Switch an extension to a remote branch.
    async fn switch_extension_branch(
        &self,
        extension_name: &str,
        branch: &str,
        global: bool,
    ) -> Result<(), DomainError>;

    /// Move an extension between local and global directories
    async fn move_extension(
        &self,
        extension_name: &str,
        source: &str,
        destination: &str,
    ) -> Result<(), DomainError>;
}
