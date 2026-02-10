use crate::domain::errors::DomainError;
use crate::domain::models::user_directory::UserDirectory;
use async_trait::async_trait;

#[async_trait]
pub trait UserDirectoryRepository: Send + Sync {
    /// Get the user directory for a specific handle
    async fn get_user_directory(&self, handle: &str) -> Result<UserDirectory, DomainError>;

    /// Get the default user directory
    async fn get_default_user_directory(&self) -> Result<UserDirectory, DomainError>;

    /// Ensure all directories for a user handle exist
    async fn ensure_user_directories_exist(&self, handle: &str) -> Result<(), DomainError>;

    /// Ensure all directories for the default user exist
    async fn ensure_default_user_directories_exist(&self) -> Result<(), DomainError>;
}
