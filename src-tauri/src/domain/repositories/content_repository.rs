use crate::domain::errors::DomainError;
use async_trait::async_trait;

/// Content type enum
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentType {
    Settings,
    Character,
    Sprites,
    Background,
    World,
    Avatar,
    Theme,
    Workflow,
    KoboldPreset,
    OpenAIPreset,
    NovelPreset,
    TextGenPreset,
    Instruct,
    Context,
    MovingUI,
    QuickReplies,
    SysPrompt,
    Reasoning,
}

/// Content item struct
#[derive(Debug, Clone)]
pub struct ContentItem {
    pub filename: String,
    pub content_type: ContentType,
}

#[async_trait]
pub trait ContentRepository: Send + Sync {
    /// Copy default content to user directory
    async fn copy_default_content_to_user(&self, user_handle: &str) -> Result<(), DomainError>;

    /// Get content index
    async fn get_content_index(&self) -> Result<Vec<ContentItem>, DomainError>;

    /// Check if default content is initialized for a user
    async fn is_default_content_initialized(&self, user_handle: &str) -> Result<bool, DomainError>;
}
