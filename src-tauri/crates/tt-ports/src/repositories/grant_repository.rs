use async_trait::async_trait;
use tt_domain::errors::DomainError;

#[async_trait]
pub trait GrantRepository: Send + Sync {
    async fn load(&self) -> Result<Vec<String>, DomainError>;

    async fn replace(&self, grants: &[String]) -> Result<(), DomainError>;
}
