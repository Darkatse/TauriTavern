use async_trait::async_trait;

use crate::domain::errors::DomainError;

#[async_trait]
pub trait DataChangeReconciler: Send + Sync {
    async fn reconcile(&self, reason: &str) -> Result<(), DomainError>;
}
