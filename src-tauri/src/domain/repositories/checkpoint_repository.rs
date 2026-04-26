use async_trait::async_trait;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::{Checkpoint, WorkspacePath};

#[async_trait]
pub trait CheckpointRepository: Send + Sync {
    async fn create_checkpoint(
        &self,
        run_id: &str,
        reason: &str,
        event_seq: u64,
        paths: &[WorkspacePath],
    ) -> Result<Checkpoint, DomainError>;
}
