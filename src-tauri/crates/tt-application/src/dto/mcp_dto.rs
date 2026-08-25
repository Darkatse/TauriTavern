use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tt_domain::models::{
    mcp::{McpProtocolVersionPreference, McpServerState, McpToolPermission},
    tool::{ToolDescriptionOverride, ToolId},
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMcpServerDto {
    pub display_name: String,
    pub endpoint: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub protocol_version: McpProtocolVersionPreference,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpRegistrationIdDto {
    pub registration_id: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMcpServerDto {
    pub registration_id: String,
    pub display_name: String,
    pub endpoint: String,
    pub headers: BTreeMap<String, String>,
    pub protocol_version: McpProtocolVersionPreference,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetMcpToolDescriptionOverrideDto {
    pub registration_id: String,
    pub native_name: String,
    #[serde(rename = "override")]
    pub override_: Option<ToolDescriptionOverride>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestMcpToolCallDto {
    pub call_id: String,
    pub registration_id: String,
    pub native_name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpTestCallIdDto {
    pub call_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpExecutionCallIdDto {
    pub execution_call_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CallLegacyMcpToolDto {
    pub execution_call_id: String,
    pub tool_id: ToolId,
    pub arguments_json: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDto {
    pub id: String,
    pub display_name: String,
    pub endpoint: String,
    pub headers: BTreeMap<String, String>,
    pub protocol_version: McpProtocolVersionPreference,
    pub state: McpServerState,
    pub tool_permissions: BTreeMap<String, McpToolPermission>,
    pub tool_description_overrides: BTreeMap<String, ToolDescriptionOverride>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStorageIssueDto {
    pub file_name: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMcpToolDto {
    pub tool_id: ToolId,
    pub native_name: String,
    pub server_display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpModelToolDiagnosticDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<ToolId>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListLegacyMcpToolsResultDto {
    pub tools: Vec<LegacyMcpToolDto>,
    pub diagnostics: Vec<McpModelToolDiagnosticDto>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpCallDiagnosticDto {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpTextContentDto {
    pub index: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum McpKnownResponseDto {
    ToolResult {
        is_error: bool,
        text_blocks: Vec<McpTextContentDto>,
        #[serde(skip_serializing_if = "Option::is_none")]
        structured_json: Option<String>,
        diagnostics: Vec<McpCallDiagnosticDto>,
    },
    ServerError {
        code: i32,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data_json: Option<String>,
    },
    UnsupportedResponse {
        response_type: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    tag = "outcome",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum McpCallOutcomeDto {
    KnownResponse { response: McpKnownResponseDto },
    NotSent { code: String, message: String },
    OutcomeUnknown { code: String, message: String },
}
