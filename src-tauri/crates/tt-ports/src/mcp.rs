use async_trait::async_trait;
use serde_json::Value;

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

#[async_trait]
pub trait McpGateway: Send + Sync {
    async fn discover_tools(
        &self,
        endpoint: &McpEndpoint,
    ) -> Result<McpDiscoveryResult, DomainError>;
}
