use async_trait::async_trait;

use super::workspace_repository::WorkspaceFile;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{Checkpoint, WorkspacePath};

#[async_trait]
pub trait CheckpointRepository: Send + Sync {
    async fn create_checkpoint(
        &self,
        run_id: &str,
        reason: &str,
        event_seq: u64,
        files: &[WorkspaceFile],
    ) -> Result<Checkpoint, DomainError>;

    async fn read_checkpoint_text(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError>;
}
