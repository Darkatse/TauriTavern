//! MCP servers reachable from inside a workspace shell.
//!
//! This is the application-side implementation of the shell's MCP port. It
//! exposes the servers the invocation already admits, and routes every call
//! through the permission-checked service path so a shell call and a model call
//! obey the same rules.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tt_domain::errors::DomainError;
use tt_domain::models::mcp::McpRegistrationId;
use tt_domain::models::tool::ToolId;
use tt_ports::mcp::{McpCallOutcome, McpKnownResponse};
use tt_ports::workspace_shell::{
    WorkspaceShellMcp, WorkspaceShellMcpOutcome, WorkspaceShellMcpServer,
};

use crate::services::mcp_service::McpService;

/// The MCP servers one invocation may reach from its shells.
pub(crate) struct ShellMcp {
    service: Arc<McpService>,
    /// Registration ids taken from the invocation's frozen tool snapshot, each
    /// paired with the tool names that snapshot exposes for it.
    servers: Vec<(McpRegistrationId, Vec<String>)>,
}

impl std::fmt::Debug for ShellMcp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellMcp")
            .field("servers", &self.servers.len())
            .finish_non_exhaustive()
    }
}

impl ShellMcp {
    pub(crate) fn new(
        service: Arc<McpService>,
        servers: Vec<(McpRegistrationId, Vec<String>)>,
    ) -> Self {
        Self { service, servers }
    }

    /// Collect the MCP tools an invocation's snapshot exposes, grouped by server.
    ///
    /// Grouping happens here because the snapshot carries flat tool ids, while
    /// the shell addresses one server at a time.
    pub(crate) fn from_snapshot(tool_ids: &[ToolId]) -> Vec<(McpRegistrationId, Vec<String>)> {
        let mut grouped: Vec<(McpRegistrationId, Vec<String>)> = Vec::new();
        for tool_id in tool_ids {
            let Ok(id) = McpRegistrationId::from_provider_id(tool_id.provider_id()) else {
                continue;
            };
            let name = tool_id.native_name().to_string();
            match grouped.iter_mut().find(|(seen, _)| *seen == id) {
                Some((_, tools)) => {
                    if !tools.contains(&name) {
                        tools.push(name);
                    }
                }
                None => grouped.push((id, vec![name])),
            }
        }
        grouped
    }
}

#[async_trait]
impl WorkspaceShellMcp for ShellMcp {
    async fn servers(&self) -> Result<Vec<WorkspaceShellMcpServer>, DomainError> {
        let mut out = Vec::with_capacity(self.servers.len());
        for (id, tools) in &self.servers {
            let name = self
                .service
                .registration_display_name(id)
                .await
                .unwrap_or_else(|| id.to_string());
            out.push(WorkspaceShellMcpServer {
                id: id.to_string(),
                name,
                tools: tools.clone(),
            });
        }
        Ok(out)
    }

    async fn call(
        &self,
        server_id: &str,
        tool: &str,
        args: Value,
    ) -> Result<WorkspaceShellMcpOutcome, DomainError> {
        let id = McpRegistrationId::parse(server_id)?;
        let tool_id = ToolId::new(&id.provider_id(), tool)?;
        let outcome = self
            .service
            .call_permitted_tool(&tool_id, args, tokio_util::sync::CancellationToken::new())
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        Ok(match outcome {
            McpCallOutcome::KnownResponse(response) => render(response),
            // The request never reached the tool, so a retry cannot repeat an effect.
            McpCallOutcome::NotSent(issue) => WorkspaceShellMcpOutcome::Refused(describe(&issue)),
            // The request was sent and the answer never arrived. Reporting this as
            // an ordinary failure would invite a retry that repeats the effect.
            McpCallOutcome::OutcomeUnknown(issue) => WorkspaceShellMcpOutcome::Unconfirmed(
                format!("{}; the remote tool may have executed", describe(&issue)),
            ),
        })
    }
}

fn describe(issue: &tt_ports::mcp::McpCallIssue) -> String {
    format!("{}: {}", issue.code, issue.message)
}

/// Render one answered MCP response for a shell caller.
///
/// A tool that reported an error still has text worth showing, so its own text
/// is preferred over a generic message. An answered error is safe to retry at
/// the transport level, so it is reported as refused rather than unconfirmed.
fn render(response: McpKnownResponse) -> WorkspaceShellMcpOutcome {
    match response {
        McpKnownResponse::ToolResult(result) => {
            let text = result
                .text
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            if result.is_error {
                let message = if text.is_empty() {
                    "mcp.tool_error: the tool reported an error".to_string()
                } else {
                    text
                };
                return WorkspaceShellMcpOutcome::Refused(message);
            }
            WorkspaceShellMcpOutcome::Answered(text)
        }
        McpKnownResponse::ServerError(error) => {
            WorkspaceShellMcpOutcome::Refused(format!("mcp.server_error: {}", error.message))
        }
        McpKnownResponse::Unsupported(response) => WorkspaceShellMcpOutcome::Refused(format!(
            "mcp.unsupported_response: {} ({})",
            response.message, response.response_type
        )),
    }
}
