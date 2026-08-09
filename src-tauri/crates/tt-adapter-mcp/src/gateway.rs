use std::{
    collections::{BTreeMap, HashSet},
    future::Future,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use rmcp::{
    ClientLifecycleMode, RoleClient,
    model::{
        CallToolRequest, CallToolRequestParams, ClientCapabilities, ClientInfo, ClientRequest,
        ContentBlock, Implementation, PaginatedRequestParams, ProtocolVersion, ResourceContents,
        ServerResult, Tool,
    },
    service::{
        ClientInitializeError, PeerRequestOptions, RunningService, ServiceError,
        serve_client_with_lifecycle_and_ct,
    },
    transport::{
        common::client_side_sse::NeverRetry,
        streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpClientWorker},
        worker::WorkerTransport,
    },
};
use serde_json::{Map, Value, json};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tt_adapter_http::{HttpClientPool, HttpClientProfile, MCP_REQUEST_TIMEOUT};

use crate::bounded_http_client::{BoundedReqwestClient, MAX_HTTP_RESPONSE_BYTES};
use tt_domain::{
    errors::DomainError,
    models::mcp::{McpEndpoint, validate_native_tool_name},
};
use tt_ports::mcp::{
    McpCallDiagnostic, McpCallIssue, McpCallOutcome, McpDiscoveredTool, McpDiscoveryResult,
    McpGateway, McpKnownResponse, McpServerError, McpTextContent, McpToolCallResult,
    McpToolDiagnostic, McpUnsupportedResponse,
};

const MAX_DISCOVERY_PAGES: usize = 32;
const MAX_DISCOVERED_TOOLS: usize = 512;
const MAX_TOOL_BYTES: usize = 256 * 1024;
const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(120);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

type McpClient = RunningService<RoleClient, ClientInfo>;

pub struct RmcpMcpGateway {
    http_clients: Arc<HttpClientPool>,
}

impl RmcpMcpGateway {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }
}

fn client_info() -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("TauriTavern", env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(ProtocolVersion::V_2025_11_25)
}

fn auto_lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: vec![
            ProtocolVersion::V_2026_07_28,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_03_26,
        ],
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    }
}

fn transport(
    endpoint: &McpEndpoint,
    http_client: reqwest::Client,
    cancel: CancellationToken,
) -> WorkerTransport<StreamableHttpClientWorker<BoundedReqwestClient>> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint.as_str());
    config.retry_config = Arc::new(NeverRetry::default());
    config.max_sse_event_size = MAX_HTTP_RESPONSE_BYTES;
    config.reinit_on_expired_session = false;
    let worker = StreamableHttpClientWorker::new(
        BoundedReqwestClient::new(http_client, MAX_HTTP_RESPONSE_BYTES, cancel.clone()),
        config,
    );
    WorkerTransport::spawn_with_ct(worker, cancel)
}

async fn serve_attempt(
    endpoint: &McpEndpoint,
    http_client: reqwest::Client,
    lifecycle: ClientLifecycleMode,
    cancel: &CancellationToken,
) -> Result<McpClient, ClientInitializeError> {
    let attempt_cancel = cancel.child_token();
    // Worker shutdown must close the channel, not masquerade as caller cancellation.
    let transport_cancel = attempt_cancel.child_token();
    serve_client_with_lifecycle_and_ct(
        client_info(),
        transport(endpoint, http_client, transport_cancel),
        lifecycle,
        attempt_cancel,
    )
    .await
}

async fn start_client(
    endpoint: &McpEndpoint,
    http_client: reqwest::Client,
    cancel: &CancellationToken,
) -> Result<McpClient, ClientInitializeError> {
    match serve_attempt(endpoint, http_client.clone(), auto_lifecycle(), cancel).await {
        // RMCP can collapse a finite SSE bootstrap error into ConnectionClosed
        // before Auto classifies the server as legacy.
        Err(error)
            if !cancel.is_cancelled()
                && (matches!(&error, ClientInitializeError::ConnectionClosed(_))
                    || matches!(
                        &error,
                        ClientInitializeError::JsonRpcError(error) if error.code.0 == -32000
                    )) =>
        {
            tracing::debug!(%error, "Trying legacy MCP lifecycle after Auto startup rejection");
            serve_attempt(
                endpoint,
                http_client,
                ClientLifecycleMode::Initialize,
                cancel,
            )
            .await
        }
        result => result,
    }
}

#[async_trait]
impl McpGateway for RmcpMcpGateway {
    async fn discover_tools(
        &self,
        endpoint: &McpEndpoint,
    ) -> Result<McpDiscoveryResult, DomainError> {
        let http_client = self.http_clients.client(HttpClientProfile::Mcp)?;
        let cancel = CancellationToken::new();
        let mut client = timeout(
            DISCOVERY_TIMEOUT,
            start_client(endpoint, http_client, &cancel),
        )
        .await
        .map_err(|_| DomainError::transient("mcp.discovery_initialize_timeout"))?
        .map_err(|error| {
            DomainError::transient(format!("mcp.discovery_initialize_failed: {error}"))
        })?;

        let peer_info = client.peer().peer_info().ok_or_else(|| {
            DomainError::InvalidData(
                "mcp.discovery_peer_info_missing: lifecycle completed without peer info"
                    .to_string(),
            )
        })?;
        let protocol_version = peer_info.protocol_version.to_string();
        let server_name = peer_info.server_info.as_ref().map(|info| info.name.clone());
        let server_version = peer_info
            .server_info
            .as_ref()
            .map(|info| info.version.clone());
        let supports_tools = peer_info.capabilities.tools.is_some();

        let raw_tools = if supports_tools {
            timeout(DISCOVERY_TIMEOUT, list_tools(client.peer(), None))
                .await
                .map_err(|_| DomainError::transient("mcp.discovery_list_timeout"))?
        } else {
            Ok(Vec::new())
        };
        let result = raw_tools.map(|tools| {
            let (tools, diagnostics) = validate_tools(tools);
            McpDiscoveryResult {
                protocol_version,
                server_name,
                server_version,
                tools,
                diagnostics,
            }
        });

        match client.close_with_timeout(CLOSE_TIMEOUT).await {
            Ok(Some(_)) => {}
            Ok(None) => tracing::warn!("Timed out closing short-lived MCP discovery client"),
            Err(error) => {
                tracing::warn!(%error, "Failed to join short-lived MCP discovery client");
            }
        }
        result
    }

    async fn call_tool(
        &self,
        endpoint: &McpEndpoint,
        native_name: &str,
        arguments: Map<String, Value>,
        cancel: CancellationToken,
    ) -> Result<McpCallOutcome, DomainError> {
        if cancel.is_cancelled() {
            return Ok(not_sent(
                "mcp.call_cancelled_before_send",
                "The tool request was cancelled before preparation started",
            ));
        }

        let http_client = self.http_clients.client(HttpClientProfile::Mcp)?;
        let mut client = match timeout(
            DISCOVERY_TIMEOUT,
            start_client(endpoint, http_client, &cancel),
        )
        .await
        {
            Err(_) => {
                cancel.cancel();
                return Ok(not_sent(
                    "mcp.call_initialize_timeout",
                    "Timed out preparing the MCP client before the tool request was sent",
                ));
            }
            Ok(Err(error)) => {
                let message = if cancel.is_cancelled() {
                    "The tool request was cancelled during MCP client preparation".to_string()
                } else {
                    format!("Failed to prepare the MCP client: {error}")
                };
                return Ok(not_sent("mcp.call_initialize_failed", message));
            }
            Ok(Ok(client)) => client,
        };

        let result = call_tool_with_client(&client, native_name, arguments, &cancel).await;
        match client.close_with_timeout(CLOSE_TIMEOUT).await {
            Ok(Some(_)) => {}
            Ok(None) => tracing::warn!("Timed out closing short-lived MCP tool-call client"),
            Err(error) => {
                tracing::warn!(%error, "Failed to join short-lived MCP tool-call client");
            }
        }
        result
    }
}

async fn call_tool_with_client(
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

async fn await_call_response(
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

fn not_sent(code: impl Into<String>, message: impl Into<String>) -> McpCallOutcome {
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

fn project_tool_result(result: rmcp::model::CallToolResult) -> McpToolCallResult {
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

async fn list_tools(
    peer: &rmcp::service::Peer<rmcp::RoleClient>,
    stop_after: Option<&str>,
) -> Result<Vec<Tool>, DomainError> {
    let mut tools = Vec::new();
    let mut catalog_bytes = 0usize;
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();

    for page_index in 0..MAX_DISCOVERY_PAGES {
        let params = cursor
            .clone()
            .map(|cursor| PaginatedRequestParams::default().with_cursor(Some(cursor)));
        let page = peer.list_tools(params).await.map_err(|error| {
            DomainError::transient(format!("mcp.discovery_list_failed: {error}"))
        })?;
        if page
            .result_type
            .as_ref()
            .is_some_and(|result_type| !result_type.is_complete())
        {
            return Err(DomainError::InvalidData(
                "mcp.discovery_result_type_unsupported: tools/list did not return a complete result"
                    .to_string(),
            ));
        }
        if tools.len().saturating_add(page.tools.len()) > MAX_DISCOVERED_TOOLS {
            return Err(DomainError::InvalidData(format!(
                "mcp.discovery_tool_limit: catalog exceeds {MAX_DISCOVERED_TOOLS} tools"
            )));
        }
        for tool in &page.tools {
            catalog_bytes = catalog_bytes.saturating_add(
                serde_json::to_vec(tool)
                    .map_err(|error| {
                        DomainError::InvalidData(format!("mcp.tool_serialize_failed: {error}"))
                    })?
                    .len(),
            );
            if catalog_bytes > MAX_CATALOG_BYTES {
                return Err(DomainError::InvalidData(format!(
                    "mcp.discovery_catalog_size_limit: catalog exceeds {MAX_CATALOG_BYTES} bytes"
                )));
            }
        }
        let target_found = stop_after.is_some_and(|native_name| {
            page.tools
                .iter()
                .any(|tool| tool.name.as_ref() == native_name)
        });
        tools.extend(page.tools);
        if target_found {
            return Ok(tools);
        }

        let Some(next_cursor) = page.next_cursor else {
            return Ok(tools);
        };
        if !seen_cursors.insert(next_cursor.clone()) {
            return Err(DomainError::InvalidData(format!(
                "mcp.discovery_cursor_cycle: server repeated cursor `{next_cursor}`"
            )));
        }
        if page_index + 1 == MAX_DISCOVERY_PAGES {
            return Err(DomainError::InvalidData(format!(
                "mcp.discovery_page_limit: catalog exceeds {MAX_DISCOVERY_PAGES} pages"
            )));
        }
        cursor = Some(next_cursor);
    }

    unreachable!("page limit exits inside the loop")
}

fn validate_tools(tools: Vec<Tool>) -> (Vec<McpDiscoveredTool>, Vec<McpToolDiagnostic>) {
    let mut groups = BTreeMap::<String, Vec<Tool>>::new();
    for tool in tools {
        groups.entry(tool.name.to_string()).or_default().push(tool);
    }

    let mut discovered = Vec::with_capacity(groups.len());
    let mut diagnostics = Vec::new();
    for (native_name, mut group) in groups {
        if group.len() > 1 {
            diagnostics.push(McpToolDiagnostic {
                code: "mcp.tool_duplicate_name".to_string(),
                native_name: Some(native_name.clone()),
                message: format!(
                    "Server returned {} tools named `{native_name}`; the whole name group was isolated",
                    group.len()
                ),
            });
            continue;
        }
        let tool = group.pop().expect("one-element tool group");
        match validate_tool(tool) {
            Ok((tool, warning)) => {
                discovered.push(tool);
                if let Some(warning) = warning {
                    diagnostics.push(McpToolDiagnostic {
                        code: warning.code.to_string(),
                        native_name: Some(native_name),
                        message: warning.message,
                    });
                }
            }
            Err(error) => diagnostics.push(McpToolDiagnostic {
                code: error.code.to_string(),
                native_name: Some(native_name),
                message: error.message,
            }),
        }
    }
    (discovered, diagnostics)
}

#[derive(Debug)]
struct ToolValidationError {
    code: &'static str,
    message: String,
}

fn validate_tool(
    tool: Tool,
) -> Result<(McpDiscoveredTool, Option<ToolValidationError>), ToolValidationError> {
    let native_name = tool.name.to_string();
    validate_native_tool_name(&native_name).map_err(|error| ToolValidationError {
        code: "mcp.tool_name_invalid",
        message: error.to_string(),
    })?;
    let encoded_size = serde_json::to_vec(&tool)
        .map_err(|error| ToolValidationError {
            code: "mcp.tool_serialize_failed",
            message: error.to_string(),
        })?
        .len();
    if encoded_size > MAX_TOOL_BYTES {
        return Err(ToolValidationError {
            code: "mcp.tool_size_limit",
            message: format!("Tool `{native_name}` exceeds {MAX_TOOL_BYTES} bytes"),
        });
    }

    let input_schema = Value::Object(tool.input_schema.as_ref().clone());
    validate_schema(&input_schema).map_err(|message| ToolValidationError {
        code: "mcp.tool_input_schema_invalid",
        message: format!("Tool `{native_name}` input schema is invalid: {message}"),
    })?;
    let mut output_warning = None;
    let output_schema = tool
        .output_schema
        .as_ref()
        .map(|schema| Value::Object(schema.as_ref().clone()))
        .and_then(|schema| match validate_schema(&schema) {
            Ok(()) => Some(schema),
            Err(message) => {
                output_warning = Some(ToolValidationError {
                    code: "mcp.tool_output_schema_invalid",
                    message: format!("Tool `{native_name}` output schema is invalid: {message}"),
                });
                None
            }
        });
    let annotations = tool
        .annotations
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| ToolValidationError {
            code: "mcp.tool_annotations_invalid",
            message: format!("Tool `{native_name}` annotations are invalid: {error}"),
        })?
        .unwrap_or_else(|| json!({}));

    Ok((
        McpDiscoveredTool {
            native_name,
            title: tool.title,
            description: tool.description.map(|value| value.into_owned()),
            input_schema,
            output_schema,
            annotations,
        },
        output_warning,
    ))
}

fn validate_schema(schema: &Value) -> Result<(), String> {
    jsonschema::draft202012::options()
        .build(schema)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        collections::HashMap,
        sync::{
            Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use rmcp::model::JsonObject;
    use serde_json::Map;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    use super::*;

    fn gateway() -> RmcpMcpGateway {
        RmcpMcpGateway::new(Arc::new(HttpClientPool::new("TauriTavern MCP test")))
    }

    fn tool(name: &'static str, schema: Value) -> Tool {
        let schema = schema.as_object().cloned().unwrap_or_default();
        Tool::new_with_raw(
            name,
            Some(Cow::Borrowed("test")),
            Arc::<JsonObject>::new(schema),
        )
    }

    #[test]
    fn invalid_and_duplicate_tools_are_isolated_without_hiding_healthy_tools() {
        let mut oversized = tool("oversized", json!({ "type": "object" }));
        oversized.description = Some(Cow::Owned("x".repeat(MAX_TOOL_BYTES)));
        let mut invalid_output = tool("invalid-output", json!({ "type": "object" }));
        invalid_output.output_schema = Some(Arc::new(Map::from_iter([(
            "type".to_string(),
            json!("not-a-json-schema-type"),
        )])));
        let tools = vec![
            tool("healthy", json!({ "type": "object" })),
            tool("broken", json!({ "type": "not-a-json-schema-type" })),
            tool("duplicate", json!({ "type": "object" })),
            tool("duplicate", json!({ "type": "object" })),
            invalid_output,
            oversized,
        ];

        let (tools, diagnostics) = validate_tools(tools);

        assert_eq!(tools.len(), 2);
        assert!(
            tools
                .iter()
                .any(|tool| tool.native_name == "invalid-output" && tool.output_schema.is_none())
        );
        assert_eq!(diagnostics.len(), 4);
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "mcp.tool_input_schema_invalid")
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "mcp.tool_duplicate_name")
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "mcp.tool_size_limit")
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "mcp.tool_output_schema_invalid")
        );
    }

    #[test]
    fn remote_schema_references_are_not_fetched() {
        let schema = json!({ "$ref": "https://example.com/schema.json" });

        assert!(validate_schema(&schema).is_err());
    }

    #[test]
    fn annotation_hints_are_preserved_as_untrusted_data() {
        let mut raw = tool("read", json!({ "type": "object" }));
        raw.annotations = Some(rmcp::model::ToolAnnotations::new().read_only(true));

        let (discovered, warning) = validate_tool(raw).unwrap();

        assert!(warning.is_none());
        assert_eq!(discovered.annotations["readOnlyHint"], true);
        assert_eq!(
            discovered.input_schema,
            Value::Object(Map::from_iter([(
                "type".to_string(),
                Value::String("object".to_string()),
            )]))
        );
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FixtureMode {
        Modern,
        ModernServerError,
        ModernHang,
        ModernDisconnect,
        ModernMalformed,
        ModernInvalidHeader,
        NoTools,
        Legacy,
        LegacyVersionRejection,
    }

    impl FixtureMode {
        fn is_modern(self) -> bool {
            matches!(
                self,
                Self::Modern
                    | Self::ModernServerError
                    | Self::ModernHang
                    | Self::ModernDisconnect
                    | Self::ModernMalformed
                    | Self::ModernInvalidHeader
            )
        }
    }

    #[derive(Debug)]
    struct FixtureRequest {
        headers: HashMap<String, String>,
        body: Value,
    }

    #[tokio::test]
    async fn streamable_http_fixture_covers_modern_lifecycle_and_full_pagination() {
        let (endpoint, requests, _, server) = spawn_fixture(FixtureMode::Modern).await;

        let result = gateway()
            .discover_tools(&McpEndpoint::parse(endpoint).unwrap())
            .await
            .unwrap();
        server.abort();

        assert_eq!(result.protocol_version, "2026-07-28");
        assert_eq!(result.server_name.as_deref(), Some("fixture-modern"));
        assert_eq!(
            result
                .tools
                .iter()
                .map(|tool| tool.native_name.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(requests.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn discovery_does_not_list_tools_without_the_server_capability() {
        let (endpoint, requests, _, server) = spawn_fixture(FixtureMode::NoTools).await;

        let result = gateway()
            .discover_tools(&McpEndpoint::parse(endpoint).unwrap())
            .await
            .unwrap();
        server.abort();

        assert!(result.tools.is_empty());
        assert_eq!(requests.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn discovery_retries_legacy_after_finite_sse_method_not_found() {
        let (endpoint, requests, _, server) = spawn_fixture(FixtureMode::Legacy).await;

        let result = gateway()
            .discover_tools(&McpEndpoint::parse(endpoint).unwrap())
            .await
            .unwrap();
        server.abort();

        assert_eq!(result.protocol_version, "2025-11-25");
        assert_eq!(result.server_name.as_deref(), Some("fixture-legacy"));
        assert_eq!(result.tools[0].native_name, "legacy_tool");
        assert!(requests.load(Ordering::Relaxed) >= 4);
    }

    #[tokio::test]
    async fn discovery_tries_legacy_lifecycle_after_generic_version_rejection() {
        let (endpoint, requests, _, server) =
            spawn_fixture(FixtureMode::LegacyVersionRejection).await;

        let result = gateway()
            .discover_tools(&McpEndpoint::parse(endpoint).unwrap())
            .await
            .unwrap();
        server.abort();

        assert_eq!(result.protocol_version, "2025-11-25");
        assert_eq!(result.server_name.as_deref(), Some("fixture-legacy"));
        assert_eq!(result.tools[0].native_name, "legacy_tool");
        assert!(requests.load(Ordering::Relaxed) >= 4);
    }

    #[tokio::test]
    async fn modern_call_hydrates_standard_headers_and_preserves_arguments() {
        let (endpoint, _, captured, server) = spawn_fixture(FixtureMode::Modern).await;
        let arguments =
            serde_json::from_str(r#"{"region":"us-east-1","exact":9007199254740993}"#).unwrap();

        let outcome = gateway()
            .call_tool(
                &McpEndpoint::parse(endpoint).unwrap(),
                "first",
                arguments,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        server.abort();

        let McpCallOutcome::KnownResponse(McpKnownResponse::ToolResult(result)) = outcome else {
            panic!("expected a known tool result");
        };
        assert!(result.is_error);
        assert_eq!(result.text[0].text, "fixture tool error");

        let requests = captured.lock().unwrap();
        let call = requests
            .iter()
            .find(|request| request.body["method"] == "tools/call")
            .expect("tools/call request");
        assert_eq!(call.body["params"]["arguments"]["region"], "us-east-1");
        assert_eq!(
            call.body["params"]["arguments"]["exact"].to_string(),
            "9007199254740993"
        );
        assert_eq!(
            call.headers.get("mcp-method").map(String::as_str),
            Some("tools/call")
        );
        assert_eq!(
            call.headers.get("mcp-name").map(String::as_str),
            Some("first")
        );
        assert_eq!(
            call.headers.get("mcp-param-region").map(String::as_str),
            Some("us-east-1")
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.body["method"] == "tools/list")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn json_rpc_tool_error_is_a_known_response() {
        let (endpoint, _, _, server) = spawn_fixture(FixtureMode::ModernServerError).await;

        let outcome = gateway()
            .call_tool(
                &McpEndpoint::parse(endpoint).unwrap(),
                "first",
                Map::from_iter([("region".to_string(), json!("us-east-1"))]),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        server.abort();

        let McpCallOutcome::KnownResponse(McpKnownResponse::ServerError(error)) = outcome else {
            panic!("expected a known JSON-RPC error");
        };
        assert_eq!(error.code, -32602);
        assert_eq!(error.message, "fixture invalid params");
        assert_eq!(error.data, Some(json!({ "field": "region" })));
    }

    #[tokio::test]
    async fn invalid_standard_header_annotation_makes_the_target_not_sent() {
        let (endpoint, _, captured, server) = spawn_fixture(FixtureMode::ModernInvalidHeader).await;

        let outcome = gateway()
            .call_tool(
                &McpEndpoint::parse(endpoint).unwrap(),
                "first",
                Map::from_iter([("region".to_string(), json!("us-east-1"))]),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        server.abort();

        assert!(matches!(outcome, McpCallOutcome::NotSent(_)));
        assert!(
            captured
                .lock()
                .unwrap()
                .iter()
                .all(|request| request.body["method"] != "tools/call")
        );
    }

    #[tokio::test]
    async fn cancelling_after_tools_call_returns_unknown_and_aborts_local_io() {
        let (endpoint, _, captured, server) = spawn_fixture(FixtureMode::ModernHang).await;
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let call = tokio::spawn(async move {
            gateway()
                .call_tool(
                    &McpEndpoint::parse(endpoint).unwrap(),
                    "first",
                    Map::from_iter([("region".to_string(), json!("us-east-1"))]),
                    task_cancel,
                )
                .await
        });
        timeout(Duration::from_secs(2), async {
            loop {
                if captured
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| request.body["method"] == "tools/call")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        cancel.cancel();
        let outcome = timeout(Duration::from_secs(3), call)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        server.abort();

        let McpCallOutcome::OutcomeUnknown(issue) = outcome else {
            panic!("expected unknown outcome after commit");
        };
        assert_eq!(issue.code, "mcp.call_cancelled");
    }

    #[tokio::test]
    async fn disconnect_and_malformed_response_after_commit_are_unknown() {
        for mode in [FixtureMode::ModernDisconnect, FixtureMode::ModernMalformed] {
            let (endpoint, _, _, server) = spawn_fixture(mode).await;
            let outcome = gateway()
                .call_tool(
                    &McpEndpoint::parse(endpoint).unwrap(),
                    "first",
                    Map::from_iter([("region".to_string(), json!("us-east-1"))]),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            server.abort();

            assert!(matches!(outcome, McpCallOutcome::OutcomeUnknown(_)));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn committed_call_timeout_is_unknown_and_stops_local_io() {
        let cancel = CancellationToken::new();
        let outcome = await_call_response(
            std::future::pending::<Result<ServerResult, ServiceError>>(),
            &cancel,
        )
        .await;

        let McpCallOutcome::OutcomeUnknown(issue) = outcome else {
            panic!("expected unknown outcome after timeout");
        };
        assert_eq!(issue.code, "mcp.call_timeout");
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn tool_result_keeps_error_text_and_reports_unsupported_blocks() {
        let result = project_tool_result(rmcp::model::CallToolResult::error(vec![
            ContentBlock::text("failed"),
            ContentBlock::image("encoded", "image/png"),
        ]));

        assert!(result.is_error);
        assert_eq!(result.text[0].text, "failed");
        assert_eq!(result.text[0].index, 0);
        assert_eq!(result.diagnostics[0].content_index, Some(1));
    }

    async fn spawn_fixture(
        mode: FixtureMode,
    ) -> (
        String,
        Arc<AtomicUsize>,
        Arc<StdMutex<Vec<FixtureRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        let captured = Arc::new(StdMutex::new(Vec::new()));
        let captured_requests = captured.clone();
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let Ok((http_method, headers, request)) = read_http_request(&mut stream).await
                else {
                    return;
                };
                request_count.fetch_add(1, Ordering::Relaxed);
                captured_requests.lock().unwrap().push(FixtureRequest {
                    headers,
                    body: request.clone(),
                });
                if mode == FixtureMode::ModernHang
                    && request.get("method").and_then(Value::as_str) == Some("tools/call")
                {
                    std::future::pending::<()>().await;
                }
                if mode == FixtureMode::ModernDisconnect
                    && request.get("method").and_then(Value::as_str) == Some("tools/call")
                {
                    continue;
                }
                if mode == FixtureMode::ModernMalformed
                    && request.get("method").and_then(Value::as_str) == Some("tools/call")
                {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json",
                        )
                        .await
                        .unwrap();
                    continue;
                }
                let sse = matches!(mode, FixtureMode::Legacy)
                    && request.get("method").and_then(Value::as_str) == Some("server/discover");
                let (status, headers, response) = fixture_response(mode, &http_method, &request);
                write_http_response(&mut stream, status, headers, response, sse)
                    .await
                    .unwrap();
            }
        });
        (endpoint, requests, captured, server)
    }

    fn fixture_response(
        mode: FixtureMode,
        http_method: &str,
        request: &Value,
    ) -> (u16, Vec<(&'static str, &'static str)>, Option<Value>) {
        if http_method == "DELETE" {
            return (204, Vec::new(), None);
        }
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        match (mode, method) {
            (FixtureMode::NoTools, "server/discover") => (
                200,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "resultType": "complete",
                        "supportedVersions": ["2026-07-28"],
                        "capabilities": {},
                        "ttlMs": 0,
                        "cacheScope": "private"
                    }
                })),
            ),
            (mode, "server/discover") if mode.is_modern() => (
                200,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "resultType": "complete",
                        "supportedVersions": ["2026-07-28", "2025-11-25"],
                        "capabilities": { "tools": {} },
                    "_meta": {
                        "io.modelcontextprotocol/serverInfo": {
                            "name": "fixture-modern",
                            "version": "1.0"
                        }
                    },
                        "ttlMs": 0,
                        "cacheScope": "private"
                    }
                })),
            ),
            (mode, "tools/list") if mode.is_modern() => {
                let cursor = request.pointer("/params/cursor").and_then(Value::as_str);
                let (name, next_cursor) = if cursor == Some("page-2") {
                    ("second", None)
                } else {
                    ("first", Some("page-2"))
                };
                let input_schema = if name == "first" {
                    json!({
                        "type": "object",
                        "properties": {
                            "region": {
                                "type": "string",
                                "x-mcp-header": if mode == FixtureMode::ModernInvalidHeader { "" } else { "Region" }
                            }
                        }
                    })
                } else {
                    json!({ "type": "object" })
                };
                (
                    200,
                    Vec::new(),
                    Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "resultType": "complete",
                            "tools": [{
                                "name": name,
                                "description": "fixture tool",
                                "inputSchema": input_schema
                            }],
                            "nextCursor": next_cursor,
                            "ttlMs": 0,
                            "cacheScope": "private"
                        }
                    })),
                )
            }
            (FixtureMode::ModernServerError, "tools/call") => (
                400,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32602,
                        "message": "fixture invalid params",
                        "data": { "field": "region" }
                    }
                })),
            ),
            (mode, "tools/call") if mode.is_modern() => (
                200,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "resultType": "complete",
                        "content": [{ "type": "text", "text": "fixture tool error" }],
                        "structuredContent": { "received": true },
                        "isError": true
                    }
                })),
            ),
            (FixtureMode::Legacy, "server/discover") => (
                200,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": "Method not found" }
                })),
            ),
            (FixtureMode::LegacyVersionRejection, "server/discover") => (
                400,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32000,
                        "message": "Bad Request: Unsupported protocol version: 2026-07-28"
                    }
                })),
            ),
            (FixtureMode::Legacy | FixtureMode::LegacyVersionRejection, "initialize") => (
                200,
                vec![("Mcp-Session-Id", "fixture-session")],
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fixture-legacy", "version": "1.0" }
                    }
                })),
            ),
            (
                FixtureMode::Legacy | FixtureMode::LegacyVersionRejection,
                "notifications/initialized",
            ) => (202, Vec::new(), None),
            (FixtureMode::Legacy | FixtureMode::LegacyVersionRejection, "tools/list") => (
                200,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [{
                            "name": "legacy_tool",
                            "inputSchema": { "type": "object" }
                        }]
                    }
                })),
            ),
            _ => (
                404,
                Vec::new(),
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": "Method not found" }
                })),
            ),
        }
    }

    async fn read_http_request(
        stream: &mut TcpStream,
    ) -> std::io::Result<(String, HashMap<String, String>, Value)> {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let header = String::from_utf8_lossy(&bytes[..header_end]);
        let method = header.split_whitespace().next().unwrap_or("").to_string();
        let headers = header
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
            .collect::<HashMap<_, _>>();
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        while bytes.len() < header_end + content_length {
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        let body = if content_length == 0 {
            Value::Null
        } else {
            serde_json::from_slice(&bytes[header_end..header_end + content_length])
                .map_err(std::io::Error::other)?
        };
        Ok((method, headers, body))
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: u16,
        headers: Vec<(&str, &str)>,
        body: Option<Value>,
        sse: bool,
    ) -> std::io::Result<()> {
        let body = body
            .map(|value| {
                if sse {
                    format!("event: message\ndata: {value}\n\n").into_bytes()
                } else {
                    serde_json::to_vec(&value).unwrap()
                }
            })
            .unwrap_or_default();
        let reason = match status {
            200 => "OK",
            202 => "Accepted",
            204 => "No Content",
            404 => "Not Found",
            _ => "Error",
        };
        let content_type = if sse {
            "text/event-stream"
        } else {
            "application/json"
        };
        let mut response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (name, value) in headers {
            response.push_str(&format!("{name}: {value}\r\n"));
        }
        response.push_str("\r\n");
        stream.write_all(response.as_bytes()).await?;
        stream.write_all(&body).await
    }
}
