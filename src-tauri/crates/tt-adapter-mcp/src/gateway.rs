use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use rmcp::{
    ClientLifecycleMode, ClientServiceExt,
    model::{
        ClientCapabilities, ClientInfo, Implementation, PaginatedRequestParams, ProtocolVersion,
        Tool,
    },
    service::ClientInitializeError,
    transport::{
        StreamableHttpClientTransport, common::client_side_sse::NeverRetry,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use tokio::time::timeout;
use tt_adapter_http::{HttpClientPool, HttpClientProfile};

use crate::bounded_http_client::{BoundedReqwestClient, MAX_HTTP_RESPONSE_BYTES};
use tt_domain::{
    errors::DomainError,
    models::mcp::{McpEndpoint, validate_native_tool_name},
};
use tt_ports::mcp::{McpDiscoveredTool, McpDiscoveryResult, McpGateway, McpToolDiagnostic};

const MAX_DISCOVERY_PAGES: usize = 32;
const MAX_DISCOVERED_TOOLS: usize = 512;
const MAX_TOOL_BYTES: usize = 256 * 1024;
const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(120);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

pub struct RmcpMcpGateway {
    http_clients: Arc<HttpClientPool>,
}

impl RmcpMcpGateway {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }
}

#[async_trait]
impl McpGateway for RmcpMcpGateway {
    async fn discover_tools(
        &self,
        endpoint: &McpEndpoint,
    ) -> Result<McpDiscoveryResult, DomainError> {
        let http_client = self.http_clients.client(HttpClientProfile::Mcp)?;
        let new_transport = || {
            let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint.as_str());
            config.retry_config = Arc::new(NeverRetry::default());
            config.max_sse_event_size = MAX_HTTP_RESPONSE_BYTES;
            config.reinit_on_expired_session = false;
            StreamableHttpClientTransport::with_client(
                BoundedReqwestClient::new(http_client.clone(), MAX_HTTP_RESPONSE_BYTES),
                config,
            )
        };
        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("TauriTavern", env!("CARGO_PKG_VERSION")),
        )
        .with_protocol_version(ProtocolVersion::V_2025_11_25);
        let lifecycle = ClientLifecycleMode::Auto {
            preferred_versions: vec![
                ProtocolVersion::V_2026_07_28,
                ProtocolVersion::V_2025_11_25,
                ProtocolVersion::V_2025_06_18,
                ProtocolVersion::V_2025_03_26,
            ],
            legacy_version: Some(ProtocolVersion::V_2025_11_25),
        };

        let startup = async {
            match client_info
                .clone()
                .serve_with_lifecycle(new_transport(), lifecycle)
                .await
            {
                // rmcp 3.1.1 can collapse a finite SSE discovery error into
                // ConnectionClosed before Auto can classify the legacy server.
                Err(error)
                    if matches!(&error, ClientInitializeError::ConnectionClosed(_))
                        || matches!(
                            &error,
                            ClientInitializeError::JsonRpcError(error) if error.code.0 == -32000
                        ) =>
                {
                    tracing::debug!(%error, "Trying legacy MCP lifecycle after Auto startup rejection");
                    client_info
                        .serve_with_lifecycle(new_transport(), ClientLifecycleMode::Initialize)
                        .await
                }
                result => result,
            }
        };
        let mut client = timeout(DISCOVERY_TIMEOUT, startup)
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
            timeout(DISCOVERY_TIMEOUT, list_all_tools(client.peer()))
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
}

async fn list_all_tools(
    peer: &rmcp::service::Peer<rmcp::RoleClient>,
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
        tools.extend(page.tools);

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
            Ok(tool) => discovered.push(tool),
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

fn validate_tool(tool: Tool) -> Result<McpDiscoveredTool, ToolValidationError> {
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
    let output_schema = tool
        .output_schema
        .as_ref()
        .map(|schema| Value::Object(schema.as_ref().clone()));
    if let Some(schema) = &output_schema {
        validate_schema(schema).map_err(|message| ToolValidationError {
            code: "mcp.tool_output_schema_invalid",
            message: format!("Tool `{native_name}` output schema is invalid: {message}"),
        })?;
    }
    let annotations = tool
        .annotations
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| ToolValidationError {
            code: "mcp.tool_annotations_invalid",
            message: format!("Tool `{native_name}` annotations are invalid: {error}"),
        })?
        .unwrap_or_else(|| json!({}));

    Ok(McpDiscoveredTool {
        native_name,
        title: tool.title,
        description: tool.description.map(|value| value.into_owned()),
        input_schema,
        output_schema,
        annotations,
    })
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
        sync::atomic::{AtomicUsize, Ordering},
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
        let tools = vec![
            tool("healthy", json!({ "type": "object" })),
            tool("broken", json!({ "type": "not-a-json-schema-type" })),
            tool("duplicate", json!({ "type": "object" })),
            tool("duplicate", json!({ "type": "object" })),
            oversized,
        ];

        let (tools, diagnostics) = validate_tools(tools);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].native_name, "healthy");
        assert_eq!(diagnostics.len(), 3);
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

        let discovered = validate_tool(raw).unwrap();

        assert_eq!(discovered.annotations["readOnlyHint"], true);
        assert_eq!(
            discovered.input_schema,
            Value::Object(Map::from_iter([(
                "type".to_string(),
                Value::String("object".to_string()),
            )]))
        );
    }

    #[derive(Clone, Copy)]
    enum FixtureMode {
        Modern,
        NoTools,
        Legacy,
        LegacyVersionRejection,
    }

    #[tokio::test]
    async fn streamable_http_fixture_covers_modern_lifecycle_and_full_pagination() {
        let (endpoint, requests, server) = spawn_fixture(FixtureMode::Modern).await;

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
        let (endpoint, requests, server) = spawn_fixture(FixtureMode::NoTools).await;

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
        let (endpoint, requests, server) = spawn_fixture(FixtureMode::Legacy).await;

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
        let (endpoint, requests, server) = spawn_fixture(FixtureMode::LegacyVersionRejection).await;

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

    async fn spawn_fixture(
        mode: FixtureMode,
    ) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let Ok((http_method, request)) = read_http_request(&mut stream).await else {
                    return;
                };
                request_count.fetch_add(1, Ordering::Relaxed);
                let sse = matches!(mode, FixtureMode::Legacy)
                    && request.get("method").and_then(Value::as_str) == Some("server/discover");
                let (status, headers, response) = fixture_response(mode, &http_method, &request);
                write_http_response(&mut stream, status, headers, response, sse)
                    .await
                    .unwrap();
            }
        });
        (endpoint, requests, server)
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
            (FixtureMode::Modern, "server/discover") => (
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
            (FixtureMode::Modern, "tools/list") => {
                let cursor = request.pointer("/params/cursor").and_then(Value::as_str);
                let (name, next_cursor) = if cursor == Some("page-2") {
                    ("second", None)
                } else {
                    ("first", Some("page-2"))
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
                                "inputSchema": { "type": "object" }
                            }],
                            "nextCursor": next_cursor,
                            "ttlMs": 0,
                            "cacheScope": "private"
                        }
                    })),
                )
            }
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

    async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<(String, Value)> {
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
        let content_length = header
            .lines()
            .find_map(|line| {
                line.split_once(':').and_then(|(name, value)| {
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
            })
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
        Ok((method, body))
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
