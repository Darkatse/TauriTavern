use async_trait::async_trait;
use crate::domain::errors::DomainError;
use crate::domain::models::theme::Theme;

/// Repository interface for managing themes
#[async_trait]
pub trait ThemeRepository: Send + Sync {
    /// Save a theme
    async fn save_theme(&self, theme: &Theme) -> Result<(), DomainError>;
    
    /// Delete a theme
    async fn delete_theme(&self, name: &str) -> Result<(), DomainError>;
}
