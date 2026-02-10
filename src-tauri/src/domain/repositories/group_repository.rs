use crate::domain::errors::DomainError;
use crate::domain::models::group::Group;
use async_trait::async_trait;

/// Repository interface for Group entities
#[async_trait]
pub trait GroupRepository: Send + Sync {
    /// Get all groups
    async fn get_all_groups(&self) -> Result<Vec<Group>, DomainError>;

    /// Get a group by ID
    async fn get_group(&self, id: &str) -> Result<Option<Group>, DomainError>;

    /// Create a new group
    async fn create_group(&self, group: &Group) -> Result<Group, DomainError>;

    /// Update an existing group
    async fn update_group(&self, group: &Group) -> Result<Group, DomainError>;

    /// Delete a group by ID
    async fn delete_group(&self, id: &str) -> Result<(), DomainError>;

    /// Get group chat file paths
    async fn get_group_chat_paths(&self) -> Result<Vec<String>, DomainError>;

    /// Clear the group cache (if implemented)
    async fn clear_cache(&self) -> Result<(), DomainError>;
}
