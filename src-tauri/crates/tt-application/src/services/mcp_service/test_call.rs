use tokio_util::sync::CancellationToken;

use crate::{dto::mcp_dto::McpCallOutcomeDto, errors::ApplicationError};
use tt_domain::models::mcp::{McpRegistrationId, McpServerState, validate_native_tool_name};

use super::{
    MAX_ARGUMENTS_JSON_BYTES, McpService,
    call::{map_call_outcome, not_sent},
};

impl McpService {
    pub async fn test_call(
        &self,
        call_id: &str,
        registration_id: &str,
        native_name: String,
        arguments_json: String,
    ) -> Result<McpCallOutcomeDto, ApplicationError> {
        let Some(cancel) = self.calls.get(call_id).await else {
            return Ok(not_sent(
                "mcp.call_not_started",
                "The test call was not prepared or was cancelled before it started",
            ));
        };
        let result = self
            .test_call_inner(registration_id, native_name, arguments_json, cancel)
            .await;
        self.calls.complete(call_id).await;
        result
    }

    pub async fn start_call(&self, call_id: &str) -> Result<(), ApplicationError> {
        self.calls.start(call_id).await
    }

    pub async fn cancel_call(&self, call_id: &str) -> Result<(), ApplicationError> {
        self.calls.cancel(call_id).await;
        Ok(())
    }

    async fn test_call_inner(
        &self,
        registration_id: &str,
        native_name: String,
        arguments_json: String,
        cancel: CancellationToken,
    ) -> Result<McpCallOutcomeDto, ApplicationError> {
        if cancel.is_cancelled() {
            return Ok(not_sent(
                "mcp.call_cancelled_before_send",
                "The tool request was cancelled before it started",
            ));
        }

        let id = match McpRegistrationId::parse(registration_id) {
            Ok(id) => id,
            Err(error) => return Ok(not_sent("mcp.call_registration_invalid", error.to_string())),
        };
        if let Err(error) = validate_native_tool_name(&native_name) {
            return Ok(not_sent("mcp.call_tool_name_invalid", error.to_string()));
        }
        if arguments_json.len() > MAX_ARGUMENTS_JSON_BYTES {
            return Ok(not_sent(
                "mcp.call_arguments_size_limit",
                format!("Arguments JSON exceeds {MAX_ARGUMENTS_JSON_BYTES} bytes"),
            ));
        }
        let arguments = match serde_json::from_str::<serde_json::Value>(&arguments_json) {
            Ok(serde_json::Value::Object(arguments)) => arguments,
            Ok(_) => {
                return Ok(not_sent(
                    "mcp.call_arguments_not_object",
                    "Arguments must be a JSON object",
                ));
            }
            Err(error) => {
                return Ok(not_sent(
                    "mcp.call_arguments_invalid_json",
                    format!("Arguments are not valid JSON: {error}"),
                ));
            }
        };

        let Some(registration) = self.repository.load(&id).await? else {
            return Ok(not_sent(
                "mcp.call_registration_not_found",
                format!("MCP registration not found: {id}"),
            ));
        };
        if registration.state() != McpServerState::Active {
            return Ok(not_sent(
                "mcp.call_server_paused",
                format!("MCP registration `{id}` must be Active before a test call"),
            ));
        }
        let outcome = self
            .gateway
            .call_tool(
                registration.endpoint(),
                registration.request_headers(),
                registration.protocol_version(),
                &native_name,
                arguments,
                cancel,
            )
            .await?;
        Ok(map_call_outcome(outcome))
    }
}
