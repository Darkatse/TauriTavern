use std::future::Future;

use rmcp::{
    model::{
        CallToolRequest, CallToolRequestParams, ClientRequest, ContentBlock, ProtocolVersion,
        ResourceContents, ServerResult,
    },
    service::{PeerRequestOptions, ServiceError},
};
use serde_json::{Map, Value};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tt_adapter_http::MCP_REQUEST_TIMEOUT;

use tt_domain::errors::DomainError;
use tt_ports::mcp::{
    McpCallDiagnostic, McpCallIssue, McpCallOutcome, McpKnownResponse, McpServerError,
    McpTextContent, McpToolCallResult, McpUnsupportedResponse,
};

use super::{DISCOVERY_TIMEOUT, client::McpClient, discovery::list_tools};

pub(super) async fn call_tool_with_client(
    client: &McpClient,
    native_name: &str,
    arguments: Map<String, Value>,
    cancel: &CancellationToken,
) -> Result<McpCallOutcome, DomainError> {
    let peer_info = client.peer().peer_info().ok_or_else(|| {
        DomainError::InternalError(
            "mcp.call_peer_info_missing: lifecycle completed without peer info".to_string(),
        )
    })?;
    if peer_info.capabilities.tools.is_none() {
        return Ok(not_sent(
            "mcp.call_tools_capability_missing",
            "The MCP server did not declare the tools capability",
        ));
    }

    if peer_info.protocol_version >= ProtocolVersion::STANDARD_HEADERS {
        let tools = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Ok(not_sent(
                    "mcp.call_cancelled_before_send",
                    "The tool request was cancelled while preparing transport metadata",
                ));
            }
            result = timeout(DISCOVERY_TIMEOUT, list_tools(client.peer(), Some(native_name))) => {
                match result {
                    Err(_) => {
                        cancel.cancel();
                        return Ok(not_sent(
                            "mcp.call_metadata_timeout",
                            "Timed out loading the current tool metadata before the tool request was sent",
                        ));
                    }
                    Ok(Err(error)) => {
                        return Ok(not_sent(
                            "mcp.call_metadata_failed",
                            format!("Failed to load current tool metadata: {error}"),
                        ));
                    }
                    Ok(Ok(tools)) => tools,
                }
            }
        };
        if !tools.iter().any(|tool| tool.name.as_ref() == native_name) {
            return Ok(not_sent(
                "mcp.call_tool_unavailable",
                format!(
                    "Tool `{native_name}` was not advertised or was rejected by the transport in this session"
                ),
            ));
        }
    }

    if cancel.is_cancelled() {
        return Ok(not_sent(
            "mcp.call_cancelled_before_send",
            "The tool request was cancelled before it was queued",
        ));
    }

    let params = CallToolRequestParams::new(native_name.to_string()).with_arguments(arguments);
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
    let handle = tokio::select! {
        biased;
        result = client.peer().send_cancellable_request(request, PeerRequestOptions::no_options()) => {
            match result {
                Ok(handle) => handle,
                Err(error) => {
                    return Ok(not_sent(
                        "mcp.call_not_queued",
                        format!("The tool request could not be queued: {error}"),
                    ));
                }
            }
        }
        _ = cancel.cancelled() => {
            return Ok(not_sent(
                "mcp.call_cancelled_before_send",
                "The tool request was cancelled before it was queued",
            ));
        }
    };

    Ok(await_call_response(handle.await_response(), cancel).await)
}

pub(super) async fn await_call_response(
    response: impl Future<Output = Result<ServerResult, ServiceError>>,
    cancel: &CancellationToken,
) -> McpCallOutcome {
    tokio::pin!(response);
    tokio::select! {
        biased;
        result = &mut response => map_call_response(result),
        _ = cancel.cancelled() => outcome_unknown(
            "mcp.call_cancelled",
            "Stopped waiting for the tool response; the remote tool may have executed",
        ),
        _ = tokio::time::sleep(MCP_REQUEST_TIMEOUT) => {
            cancel.cancel();
            outcome_unknown(
                "mcp.call_timeout",
                "Timed out waiting for the tool response; the remote tool may have executed",
            )
        }
    }
}

pub(super) fn not_sent(code: impl Into<String>, message: impl Into<String>) -> McpCallOutcome {
    McpCallOutcome::NotSent(McpCallIssue {
        code: code.into(),
        message: message.into(),
    })
}

fn outcome_unknown(code: impl Into<String>, message: impl Into<String>) -> McpCallOutcome {
    McpCallOutcome::OutcomeUnknown(McpCallIssue {
        code: code.into(),
        message: message.into(),
    })
}

fn map_call_response(result: Result<ServerResult, ServiceError>) -> McpCallOutcome {
    match result {
        Ok(ServerResult::CallToolResult(result)) => {
            McpCallOutcome::KnownResponse(McpKnownResponse::ToolResult(project_tool_result(result)))
        }
        Ok(ServerResult::InputRequiredResult(_)) => {
            McpCallOutcome::KnownResponse(McpKnownResponse::Unsupported(McpUnsupportedResponse {
                response_type: "input_required".to_string(),
                message:
                    "The server requested additional input; TauriTavern did not continue the call"
                        .to_string(),
            }))
        }
        Ok(ServerResult::CreateTaskResult(_)) => {
            McpCallOutcome::KnownResponse(McpKnownResponse::Unsupported(McpUnsupportedResponse {
                response_type: "task".to_string(),
                message: "The server created a task; TauriTavern did not poll or continue it"
                    .to_string(),
            }))
        }
        Ok(_) => {
            McpCallOutcome::KnownResponse(McpKnownResponse::Unsupported(McpUnsupportedResponse {
                response_type: "other".to_string(),
                message: "The server returned a response type that TauriTavern does not support"
                    .to_string(),
            }))
        }
        Err(ServiceError::McpError(error)) => {
            McpCallOutcome::KnownResponse(McpKnownResponse::ServerError(McpServerError {
                code: error.code.0,
                message: error.message.into_owned(),
                data: error.data,
            }))
        }
        Err(error) => outcome_unknown(
            "mcp.call_response_failed",
            format!(
                "The tool response could not be confirmed: {error}; the remote tool may have executed"
            ),
        ),
    }
}

pub(super) fn project_tool_result(result: rmcp::model::CallToolResult) -> McpToolCallResult {
    let mut text = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, content) in result.content.into_iter().enumerate() {
        match content {
            ContentBlock::Text(content) => text.push(McpTextContent {
                index,
                text: content.text,
            }),
            ContentBlock::Image(content) => diagnostics.push(McpCallDiagnostic {
                code: "mcp.call_content_unsupported".to_string(),
                message: format!(
                    "Image content ({}, {} encoded bytes) is not supported",
                    content.mime_type,
                    content.data.len()
                ),
                content_index: Some(index),
            }),
            ContentBlock::Audio(content) => diagnostics.push(McpCallDiagnostic {
                code: "mcp.call_content_unsupported".to_string(),
                message: format!(
                    "Audio content ({}, {} encoded bytes) is not supported",
                    content.mime_type,
                    content.data.len()
                ),
                content_index: Some(index),
            }),
            ContentBlock::Resource(content) => {
                let size = match &content.resource {
                    ResourceContents::TextResourceContents { text, .. } => Some(text.len()),
                    ResourceContents::BlobResourceContents { blob, .. } => Some(blob.len()),
                    _ => None,
                };
                diagnostics.push(McpCallDiagnostic {
                    code: "mcp.call_content_unsupported".to_string(),
                    message: size.map_or_else(
                        || "Embedded resource content is not supported".to_string(),
                        |size| {
                            format!(
                                "Embedded resource content ({size} encoded bytes) is not supported"
                            )
                        },
                    ),
                    content_index: Some(index),
                });
            }
            ContentBlock::ResourceLink(content) => diagnostics.push(McpCallDiagnostic {
                code: "mcp.call_content_unsupported".to_string(),
                message: match content.size {
                    Some(size) => format!("Resource link ({size} bytes) is not supported"),
                    None => "Resource link content is not supported".to_string(),
                },
                content_index: Some(index),
            }),
            _ => diagnostics.push(McpCallDiagnostic {
                code: "mcp.call_content_unsupported".to_string(),
                message: "Unknown content type is not supported".to_string(),
                content_index: Some(index),
            }),
        }
    }
    if result.meta.is_some() {
        diagnostics.push(McpCallDiagnostic {
            code: "mcp.call_metadata_unsupported".to_string(),
            message: "Result metadata is not supported".to_string(),
            content_index: None,
        });
    }

    McpToolCallResult {
        is_error: result.is_error.unwrap_or(false),
        text,
        structured_content: result.structured_content,
        diagnostics,
    }
}
