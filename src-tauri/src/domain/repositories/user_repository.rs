use async_trait::async_trait;
use crate::domain::models::user::User;
use crate::domain::errors::DomainError;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn save(&self, user: &User) -> Result<(), DomainError>;
    async fn find_by_id(&self, id: &str) -> Result<User, DomainError>;
    async fn find_by_username(&self, username: &str) -> Result<User, DomainError>;
    async fn find_all(&self) -> Result<Vec<User>, DomainError>;
    async fn delete(&self, id: &str) -> Result<(), DomainError>;
    async fn update(&self, user: &User) -> Result<(), DomainError>;
}
