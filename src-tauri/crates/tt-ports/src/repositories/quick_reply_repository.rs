use async_trait::async_trait;

use tt_domain::errors::DomainError;
use tt_domain::models::quick_reply::QuickReplySet;

#[async_trait]
pub trait QuickReplyRepository: Send + Sync {
    async fn save_quick_reply_set(&self, set: &QuickReplySet) -> Result<(), DomainError>;
    async fn delete_quick_reply_set(&self, name: &str) -> Result<(), DomainError>;
}
