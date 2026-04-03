use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptCacheKey {
    Claude,
    OpenRouterClaude,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDigestSnapshot {
    pub version: u32,
    pub digests: Vec<String>,
}

#[async_trait]
pub trait PromptCacheRepository: Send + Sync {
    async fn load_prompt_digests(
        &self,
        key: PromptCacheKey,
    ) -> Result<Option<PromptDigestSnapshot>, DomainError>;

    async fn save_prompt_digests(
        &self,
        key: PromptCacheKey,
        snapshot: PromptDigestSnapshot,
    ) -> Result<(), DomainError>;
}
