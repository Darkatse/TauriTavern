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
    /// MCP servers this invocation may reach from inside the shell.
    ///
    /// Kept apart from `tools`: MCP exposes a fixed set of commands that take a
    /// server and tool name as arguments, rather than one command per tool.
    pub mcp: Option<Arc<dyn WorkspaceShellMcp>>,
}

impl Default for WorkspaceShellContext {
    fn default() -> Self {
        Self {
            frozen_macros: Arc::default(),
            host: Err(
                "This run has no JavaScript host context. Workspace files remain available.".into(),
            ),
            tools: None,
            mcp: None,
        }
    }
}

/// One MCP server as the shell sees it, including its two stable handles.
#[derive(Debug, Clone)]
pub struct WorkspaceShellMcpServer {
    /// Canonical registration id; unique and machine-friendly.
    pub id: String,
    /// User-chosen name; readable, but neither unique nor permanent.
    pub name: String,
    /// Tool names this server advertises in the frozen snapshot.
    pub tools: Vec<String>,
}

/// The outcome of one MCP tool call.
#[derive(Debug)]
pub enum WorkspaceShellMcpOutcome {
    /// The tool answered. The text is what the tool produced.
    Answered(String),
    /// The request was never sent, or the server refused it outright.
    ///
    /// Running the call again is safe.
    Refused(String),
    /// The call was sent but no answer was confirmed: it timed out, was
    /// cancelled, or the response was lost. The remote tool **may have run**.
    ///
    /// Running the call again may repeat its effect.
    Unconfirmed(String),
}

/// The MCP capability of one invocation, reachable as shell commands.
///
/// Server selection happens inside these calls so the shell does not have to
/// name a server in command position.
#[async_trait]
pub trait WorkspaceShellMcp: std::fmt::Debug + Send + Sync {
    /// Servers and tools this invocation may reach, in a stable order.
    async fn servers(&self) -> Result<Vec<WorkspaceShellMcpServer>, DomainError>;

    /// Call one tool with a JSON object; the caller resolved the server already.
    ///
    /// Returns how far the call got, because whether it is safe to run again
    /// cannot be recovered from an error message.
    async fn call(
        &self,
        server_id: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<WorkspaceShellMcpOutcome, DomainError>;
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
