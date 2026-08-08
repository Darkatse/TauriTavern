use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tt_domain::models::{
    mcp::{McpServerState, McpToolPermission},
    tool::ToolId,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMcpServerDto {
    pub display_name: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpRegistrationIdDto {
    pub registration_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameMcpServerDto {
    pub registration_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetMcpServerStateDto {
    pub registration_id: String,
    pub state: McpServerState,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetMcpToolPermissionDto {
    pub registration_id: String,
    pub native_name: String,
    pub permission: McpToolPermission,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDto {
    pub id: String,
    pub display_name: String,
    pub endpoint: String,
    pub state: McpServerState,
    pub tool_permissions: BTreeMap<String, McpToolPermission>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStorageIssueDto {
    pub file_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMcpServersResultDto {
    pub servers: Vec<McpServerDto>,
    pub storage_issues: Vec<McpStorageIssueDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDto {
    pub id: ToolId,
    pub native_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub annotations: Value,
    pub permission: McpToolPermission,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDiagnosticDto {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStaleToolDto {
    pub native_name: String,
    pub permission: McpToolPermission,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiscoveryResultDto {
    pub registration_id: String,
    pub protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    pub tools: Vec<McpToolDto>,
    pub diagnostics: Vec<McpToolDiagnosticDto>,
    pub stale_tools: Vec<McpStaleToolDto>,
}
