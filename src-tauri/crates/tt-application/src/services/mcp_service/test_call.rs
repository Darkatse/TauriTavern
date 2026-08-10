use std::collections::{HashMap, hash_map::Entry};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    dto::mcp_dto::{
        McpCallDiagnosticDto, McpKnownResponseDto, McpTestCallOutcomeDto, McpTextContentDto,
    },
    errors::ApplicationError,
};
use tt_domain::models::mcp::{McpRegistrationId, McpServerState, validate_native_tool_name};
use tt_ports::mcp::{McpCallOutcome, McpKnownResponse};

use super::{MAX_ARGUMENTS_JSON_BYTES, McpService};

impl McpService {
    pub async fn test_call(
        &self,
        call_id: &str,
        registration_id: &str,
        native_name: String,
        arguments_json: String,
    ) -> Result<McpTestCallOutcomeDto, ApplicationError> {
        let Some(cancel) = self.test_calls.get(call_id).await else {
            return Ok(not_sent(
                "mcp.call_not_started",
                "The test call was not prepared or was cancelled before it started",
            ));
        };
        let result = self
            .test_call_inner(registration_id, native_name, arguments_json, cancel)
            .await;
        self.test_calls.complete(call_id).await;
        result
    }

    pub async fn start_test_call(&self, call_id: &str) -> Result<(), ApplicationError> {
        self.test_calls.start(call_id).await
    }

    pub async fn cancel_test_call(&self, call_id: &str) -> Result<(), ApplicationError> {
        self.test_calls.cancel(call_id).await;
        Ok(())
    }

    async fn test_call_inner(
        &self,
        registration_id: &str,
        native_name: String,
        arguments_json: String,
        cancel: CancellationToken,
    ) -> Result<McpTestCallOutcomeDto, ApplicationError> {
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
            .call_tool(registration.endpoint(), &native_name, arguments, cancel)
            .await?;
        Ok(map_call_outcome(outcome))
    }
}

fn not_sent(code: impl Into<String>, message: impl Into<String>) -> McpTestCallOutcomeDto {
    McpTestCallOutcomeDto::NotSent {
        code: code.into(),
        message: message.into(),
    }
}

fn map_call_outcome(outcome: McpCallOutcome) -> McpTestCallOutcomeDto {
    match outcome {
        McpCallOutcome::KnownResponse(response) => McpTestCallOutcomeDto::KnownResponse {
            response: match response {
                McpKnownResponse::ToolResult(result) => McpKnownResponseDto::ToolResult {
                    is_error: result.is_error,
                    text_blocks: result
                        .text
                        .into_iter()
                        .map(|content| McpTextContentDto {
                            index: content.index,
                            text: content.text,
                        })
                        .collect(),
                    structured_json: result.structured_content.map(|value| {
                        serde_json::to_string_pretty(&value)
                            .expect("serde_json::Value is always serializable")
                    }),
                    diagnostics: result
                        .diagnostics
                        .into_iter()
                        .map(|diagnostic| McpCallDiagnosticDto {
                            code: diagnostic.code,
                            message: diagnostic.message,
                            content_index: diagnostic.content_index,
                        })
                        .collect(),
                },
                McpKnownResponse::ServerError(error) => McpKnownResponseDto::ServerError {
                    code: error.code,
                    message: error.message,
                    data_json: error.data.map(|value| value.to_string()),
                },
                McpKnownResponse::Unsupported(response) => {
                    McpKnownResponseDto::UnsupportedResponse {
                        response_type: response.response_type,
                        message: response.message,
                    }
                }
            },
        },
        McpCallOutcome::NotSent(issue) => McpTestCallOutcomeDto::NotSent {
            code: issue.code,
            message: issue.message,
        },
        McpCallOutcome::OutcomeUnknown(issue) => McpTestCallOutcomeDto::OutcomeUnknown {
            code: issue.code,
            message: issue.message,
        },
    }
}

#[derive(Default)]
pub(super) struct TestCallRegistry {
    pub(super) calls: Mutex<HashMap<String, CancellationToken>>,
}

impl TestCallRegistry {
    async fn start(&self, call_id: &str) -> Result<(), ApplicationError> {
        let mut calls = self.calls.lock().await;
        match calls.entry(call_id.to_string()) {
            Entry::Vacant(entry) => {
                entry.insert(CancellationToken::new());
                Ok(())
            }
            Entry::Occupied(_) => Err(ApplicationError::Conflict(format!(
                "mcp.call_id_in_use: call `{call_id}` is already active"
            ))),
        }
    }

    async fn get(&self, call_id: &str) -> Option<CancellationToken> {
        self.calls.lock().await.get(call_id).cloned()
    }

    async fn cancel(&self, call_id: &str) {
        if let Some(cancel) = self.calls.lock().await.remove(call_id) {
            cancel.cancel();
        }
    }

    async fn complete(&self, call_id: &str) {
        self.calls.lock().await.remove(call_id);
    }
}
