use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::watch;
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::FrozenMacros;

use crate::workspace_fs::WorkspaceFs;

pub struct WorkspaceShellRequest {
    pub command: String,
    pub workdir: String,
    pub files: Arc<dyn WorkspaceFs>,
    pub context: Arc<WorkspaceShellContext>,
    pub cancel: watch::Receiver<bool>,
}

/// Frozen host facts, separate from the ability to execute ordinary workspace code.
#[derive(Debug)]
pub struct WorkspaceShellContext {
    pub frozen_macros: Arc<FrozenMacros>,
    pub host: Result<serde_json::Value, String>,
    /// Builtin Agent tools this invocation may reach from inside the shell.
    ///
    /// Absent for shells that have no invocation behind them, such as the
    /// JavaScript probe harness.
    pub tools: Option<Arc<dyn WorkspaceShellTools>>,
}

impl Default for WorkspaceShellContext {
    fn default() -> Self {
        Self {
            frozen_macros: Arc::default(),
            host: Err(
                "This run has no JavaScript host context. Workspace files remain available.".into(),
            ),
            tools: None,
        }
    }
}

/// One invocation's builtin Agent tools, reachable as shell commands.
///
/// The visible set is fixed when the invocation is compiled, so a command that
/// is absent here does not exist inside the shell either.
#[async_trait]
pub trait WorkspaceShellTools: std::fmt::Debug + Send + Sync {
    /// Call one visible tool by native name, such as `chat.search`.
    ///
    /// Returns the text the tool produced for the model.
    async fn call(&self, name: &str, args: serde_json::Value) -> Result<String, DomainError>;

    /// Native names of the tools this invocation exposes to the shell.
    fn visible(&self) -> &[String];
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
/// it stops further interpreter scheduling and awaits started JavaScript and
/// file operations before returning.
/// Dropping this future is not a cancellation mechanism.
#[async_trait]
pub trait WorkspaceShell: Send + Sync {
    async fn execute(
        &self,
        request: WorkspaceShellRequest,
    ) -> Result<WorkspaceShellResult, DomainError>;
}
