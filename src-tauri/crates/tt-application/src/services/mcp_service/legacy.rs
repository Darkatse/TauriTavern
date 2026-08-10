use crate::{
    dto::mcp_dto::{
        LegacyMcpToolDto, ListLegacyMcpToolsResultDto, McpCallOutcomeDto, McpModelToolDiagnosticDto,
    },
    errors::ApplicationError,
};
use tt_domain::models::tool::ToolId;

use super::{
    MAX_ARGUMENTS_JSON_BYTES, McpService,
    call::{map_call_outcome, not_sent},
};

impl McpService {
    pub async fn list_legacy_tools_cached(
        &self,
    ) -> Result<ListLegacyMcpToolsResultDto, ApplicationError> {
        let resolution = self.list_permitted_model_tools_cached().await?;
        Ok(ListLegacyMcpToolsResultDto {
            tools: resolution
                .tools
                .into_iter()
                .map(|tool| LegacyMcpToolDto {
                    tool_id: tool.descriptor.id.clone(),
                    native_name: tool.descriptor.id.native_name().to_string(),
                    server_display_name: tool.server_display_name,
                    title: tool.descriptor.title,
                    description: tool.descriptor.description,
                    input_schema: tool.descriptor.input_schema,
                })
                .collect(),
            diagnostics: resolution
                .diagnostics
                .into_iter()
                .map(|diagnostic| McpModelToolDiagnosticDto {
                    tool_id: diagnostic.tool_id,
                    code: diagnostic.code,
                    message: diagnostic.message,
                })
                .collect(),
        })
    }

    pub async fn call_legacy_tool(
        &self,
        execution_call_id: &str,
        tool_id: &ToolId,
        arguments_json: String,
    ) -> Result<McpCallOutcomeDto, ApplicationError> {
        let Some(cancel) = self.calls.get(execution_call_id).await else {
            return Ok(not_sent(
                "mcp.call_not_started",
                "The Legacy tool call was not prepared or was cancelled before it started",
            ));
        };
        let result = self
            .call_legacy_tool_inner(tool_id, arguments_json, cancel)
            .await;
        self.calls.complete(execution_call_id).await;
        result
    }

    async fn call_legacy_tool_inner(
        &self,
        tool_id: &ToolId,
        arguments_json: String,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<McpCallOutcomeDto, ApplicationError> {
        if arguments_json.len() > MAX_ARGUMENTS_JSON_BYTES {
            return Ok(not_sent(
                "mcp.call_arguments_size_limit",
                format!("Arguments JSON exceeds {MAX_ARGUMENTS_JSON_BYTES} bytes"),
            ));
        }
        let arguments = if arguments_json.is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str(&arguments_json) {
                Ok(arguments) => arguments,
                Err(error) => {
                    return Ok(not_sent(
                        "mcp.call_arguments_invalid_json",
                        format!("Arguments are not valid JSON: {error}"),
                    ));
                }
            }
        };
        let outcome = self.call_permitted_tool(tool_id, arguments, cancel).await?;
        Ok(map_call_outcome(outcome))
    }
}
