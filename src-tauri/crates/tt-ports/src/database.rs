use async_trait::async_trait;
use tt_contracts::database::{DatabaseRequest, DatabaseResponse};
use tt_domain::errors::DomainError;

#[async_trait]
pub trait DatabaseBackend: Send + Sync {
    async fn execute(&self, request: DatabaseRequest) -> Result<DatabaseResponse, DomainError>;
}

/// Hold exclusive database access while Sync reads or replaces native files.
#[async_trait]
pub trait DatabaseFileAccess: Send + Sync {
    async fn prepare_sync(
        &self,
        receiving: bool,
    ) -> Result<tokio::sync::OwnedRwLockWriteGuard<()>, DomainError>;
}
