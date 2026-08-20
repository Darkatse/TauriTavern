use async_trait::async_trait;
use tt_domain::errors::DomainError;

#[async_trait]
pub trait UserEndpointGrantRepository: Send + Sync {
    async fn load(&self) -> Result<Vec<String>, DomainError>;

    async fn replace(&self, endpoints: &[String]) -> Result<(), DomainError>;
}
