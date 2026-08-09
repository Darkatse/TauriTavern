use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use tt_domain::{errors::DomainError, models::mcp::McpEndpoint};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDiscoveredTool {
    pub native_name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub annotations: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolDiagnostic {
    pub code: String,
    pub native_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDiscoveryResult {
    pub protocol_version: String,
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    pub tools: Vec<McpDiscoveredTool>,
    pub diagnostics: Vec<McpToolDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallDiagnostic {
    pub code: String,
    pub message: String,
    pub content_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTextContent {
    pub index: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolCallResult {
    pub is_error: bool,
    pub text: Vec<McpTextContent>,
    pub structured_content: Option<Value>,
    pub diagnostics: Vec<McpCallDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpUnsupportedResponse {
    pub response_type: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpKnownResponse {
    ToolResult(McpToolCallResult),
    ServerError(McpServerError),
    Unsupported(McpUnsupportedResponse),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpCallOutcome {
    KnownResponse(McpKnownResponse),
    NotSent(McpCallIssue),
    OutcomeUnknown(McpCallIssue),
}

#[async_trait]
pub trait McpGateway: Send + Sync {
    async fn discover_tools(
        &self,
        endpoint: &McpEndpoint,
    ) -> Result<McpDiscoveryResult, DomainError>;

    async fn call_tool(
        &self,
        endpoint: &McpEndpoint,
        native_name: &str,
        arguments: Map<String, Value>,
        cancel: CancellationToken,
    ) -> Result<McpCallOutcome, DomainError>;
}
