use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::watch;
use tt_domain::errors::DomainError;

use crate::workspace_fs::WorkspaceFs;

pub struct WorkspaceShellRequest {
    pub command: String,
    pub workdir: String,
    pub files: Arc<dyn WorkspaceFs>,
    pub cancel: watch::Receiver<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceShellExit {
    Exited(i32),
    Cancelled,
    TimedOut,
    Failed,
}

#[derive(Debug)]
pub struct WorkspaceShellResult {
    pub stdout: String,
    pub stderr: String,
    pub exit: WorkspaceShellExit,
    pub output_truncated: bool,
}

/// Execute one independent shell against the supplied workspace. Completed
/// file operations remain visible even when a later command fails or is cancelled.
/// Request cancellation through `request.cancel` and keep awaiting `execute`:
/// it stops the interpreter and finishes started mutations before returning.
/// Dropping this future is not a cancellation mechanism.
#[async_trait]
pub trait WorkspaceShell: Send + Sync {
    async fn execute(
        &self,
        request: WorkspaceShellRequest,
    ) -> Result<WorkspaceShellResult, DomainError>;
}
