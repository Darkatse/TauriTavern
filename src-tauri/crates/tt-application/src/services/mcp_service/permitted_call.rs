use tokio_util::sync::CancellationToken;

use crate::errors::ApplicationError;
use tt_domain::models::{
    mcp::{McpRegistrationId, McpServerState, McpToolPermission, validate_native_tool_name},
    tool::ToolId,
};
use tt_ports::mcp::{McpCallIssue, McpCallOutcome};

use super::{MAX_ARGUMENTS_JSON_BYTES, McpService};

impl McpService {
    pub(crate) async fn call_permitted_tool(
        &self,
        tool_id: &ToolId,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<McpCallOutcome, ApplicationError> {
        if cancel.is_cancelled() {
            return Ok(not_sent(
                "mcp.call_cancelled_before_send",
                "The tool request was cancelled before it started",
            ));
        }
        let registration_id = McpRegistrationId::from_provider_id(tool_id.provider_id())?;
        validate_native_tool_name(tool_id.native_name())?;
        let arguments_bytes = serde_json::to_vec(&arguments).map_err(|error| {
            ApplicationError::ValidationError(format!("mcp.call_arguments_invalid_json: {error}"))
        })?;
        if arguments_bytes.len() > MAX_ARGUMENTS_JSON_BYTES {
            return Ok(not_sent(
                "mcp.call_arguments_size_limit",
                format!("Arguments JSON exceeds {MAX_ARGUMENTS_JSON_BYTES} bytes"),
            ));
        }
        let serde_json::Value::Object(arguments) = arguments else {
            return Ok(not_sent(
                "mcp.call_arguments_not_object",
                "Arguments must be a JSON object",
            ));
        };
        let registration = match self.repository.load(&registration_id).await {
            Ok(Some(registration)) => registration,
            Ok(None) => {
                return Ok(not_sent(
                    "mcp.call_registration_not_found",
                    format!("MCP registration not found: {registration_id}"),
                ));
            }
            Err(error) => {
                return Ok(not_sent(
                    "mcp.call_registration_unavailable",
                    error.to_string(),
                ));
            }
        };
        if registration.state() != McpServerState::Active {
            return Ok(not_sent(
                "mcp.call_server_paused",
                format!("MCP server `{}` is paused", registration.display_name()),
            ));
        }
        if registration.permission_for(tool_id.native_name()) == McpToolPermission::Off {
            return Ok(not_sent(
                "mcp.call_permission_off",
                format!("MCP tool `{tool_id}` is Off"),
            ));
        }
        Ok(self
            .gateway
            .call_tool(
                registration.endpoint(),
                registration.request_headers(),
                registration.protocol_version(),
                tool_id.native_name(),
                arguments,
                cancel,
            )
            .await?)
    }
}

fn not_sent(code: impl Into<String>, message: impl Into<String>) -> McpCallOutcome {
    McpCallOutcome::NotSent(McpCallIssue {
        code: code.into(),
        message: message.into(),
    })
}
