use async_trait::async_trait;
use serde_json::Value;

use crate::domain::errors::DomainError;

#[async_trait]
pub trait TokenizerRepository: Send + Sync {
    async fn ensure_model_ready(&self, model: &str) -> Result<(), DomainError>;

    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError>;

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError>;

    fn count_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError>;
}
