use async_trait::async_trait;

use crate::domain::errors::DomainError;
use crate::domain::models::llm_connection::{
    LlmConnectionDefinition, LlmConnectionId, LlmConnectionSummary,
};

#[async_trait]
pub trait LlmConnectionRepository: Send + Sync {
    async fn list_connections(&self) -> Result<Vec<LlmConnectionSummary>, DomainError>;

    async fn load_connection(
        &self,
        id: &LlmConnectionId,
    ) -> Result<Option<LlmConnectionDefinition>, DomainError>;

    async fn save_connection(
        &self,
        connection: &LlmConnectionDefinition,
    ) -> Result<(), DomainError>;

    async fn delete_connection(&self, id: &LlmConnectionId) -> Result<(), DomainError>;
}
