use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;

/// Responses are the Horde's JSON as received.
#[async_trait]
pub trait HordeRepository: Send + Sync {
    async fn heartbeat(&self) -> Result<(), DomainError>;

    async fn text_workers(&self) -> Result<Value, DomainError>;

    async fn text_models(&self) -> Result<Value, DomainError>;

    async fn text_model_reference(&self) -> Result<Value, DomainError>;

    async fn find_user(&self, api_key: &str) -> Result<Value, DomainError>;

    /// `None` when `api_key` is not a shared key.
    async fn shared_key(&self, api_key: &str) -> Result<Option<Value>, DomainError>;

    async fn submit_text_generation(
        &self,
        body: &Value,
        api_key: &str,
    ) -> Result<Value, DomainError>;

    async fn text_generation_status(&self, task_id: &str) -> Result<Value, DomainError>;

    async fn cancel_text_generation(&self, task_id: &str) -> Result<(), DomainError>;
}
