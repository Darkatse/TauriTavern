use std::collections::{HashMap, hash_map::Entry};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    dto::mcp_dto::{
        McpCallDiagnosticDto, McpCallOutcomeDto, McpKnownResponseDto, McpTextContentDto,
    },
    errors::ApplicationError,
};
use tt_ports::mcp::{McpCallOutcome, McpKnownResponse};

pub(super) fn not_sent(code: impl Into<String>, message: impl Into<String>) -> McpCallOutcomeDto {
    McpCallOutcomeDto::NotSent {
        code: code.into(),
        message: message.into(),
    }
}

pub(super) fn map_call_outcome(outcome: McpCallOutcome) -> McpCallOutcomeDto {
    match outcome {
        McpCallOutcome::KnownResponse(response) => McpCallOutcomeDto::KnownResponse {
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
        McpCallOutcome::NotSent(issue) => McpCallOutcomeDto::NotSent {
            code: issue.code,
            message: issue.message,
        },
        McpCallOutcome::OutcomeUnknown(issue) => McpCallOutcomeDto::OutcomeUnknown {
            code: issue.code,
            message: issue.message,
        },
    }
}

#[derive(Default)]
pub(super) struct CallRegistry {
    pub(super) calls: Mutex<HashMap<String, CancellationToken>>,
}

impl CallRegistry {
    pub(super) async fn start(&self, call_id: &str) -> Result<(), ApplicationError> {
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

    pub(super) async fn get(&self, call_id: &str) -> Option<CancellationToken> {
        self.calls.lock().await.get(call_id).cloned()
    }

    pub(super) async fn cancel(&self, call_id: &str) {
        if let Some(cancel) = self.calls.lock().await.remove(call_id) {
            cancel.cancel();
        }
    }

    pub(super) async fn complete(&self, call_id: &str) {
        self.calls.lock().await.remove(call_id);
    }
}
