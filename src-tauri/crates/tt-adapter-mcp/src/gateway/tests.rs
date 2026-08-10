use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use rmcp::{
    model::{ContentBlock, JsonObject, ServerResult, Tool},
    service::ServiceError,
};
use serde_json::{Map, Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use tokio_util::sync::CancellationToken;
use tt_adapter_http::HttpClientPool;
use tt_domain::models::mcp::{McpEndpoint, McpProtocolVersionPreference, McpRequestHeaders};
use tt_ports::mcp::{McpCallOutcome, McpGateway, McpKnownResponse};

use super::{
    RmcpMcpGateway,
    discovery::{MAX_TOOL_BYTES, validate_schema, validate_tool, validate_tools},
    tool_call::{await_call_response, project_tool_result},
};

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
        .discover_tools(
            &McpEndpoint::parse(endpoint).unwrap(),
            &McpRequestHeaders::default(),
            McpProtocolVersionPreference::Auto,
        )
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
        .discover_tools(
            &McpEndpoint::parse(endpoint).unwrap(),
            &McpRequestHeaders::default(),
            McpProtocolVersionPreference::Auto,
        )
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
        .discover_tools(
            &McpEndpoint::parse(endpoint).unwrap(),
            &McpRequestHeaders::default(),
            McpProtocolVersionPreference::Auto,
        )
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
    let (endpoint, requests, _, server) = spawn_fixture(FixtureMode::LegacyVersionRejection).await;

    let result = gateway()
        .discover_tools(
            &McpEndpoint::parse(endpoint).unwrap(),
            &McpRequestHeaders::default(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    server.abort();

    assert_eq!(result.protocol_version, "2025-11-25");
    assert_eq!(result.server_name.as_deref(), Some("fixture-legacy"));
    assert_eq!(result.tools[0].native_name, "legacy_tool");
    assert!(requests.load(Ordering::Relaxed) >= 4);
}

#[tokio::test]
async fn modern_call_sends_custom_and_standard_headers_and_preserves_arguments() {
    let (endpoint, _, captured, server) = spawn_fixture(FixtureMode::Modern).await;
    let request_headers = McpRequestHeaders::from(BTreeMap::from([(
        "x-api-key".to_string(),
        "fixture-secret".to_string(),
    )]));
    let arguments =
        serde_json::from_str(r#"{"region":"us-east-1","exact":9007199254740993}"#).unwrap();

    let outcome = gateway()
        .call_tool(
            &McpEndpoint::parse(endpoint).unwrap(),
            &request_headers,
            McpProtocolVersionPreference::Auto,
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
    assert!(requests.iter().all(|request| {
        request.headers.get("x-api-key").map(String::as_str) == Some("fixture-secret")
    }));
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
            &McpRequestHeaders::default(),
            McpProtocolVersionPreference::Auto,
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
            &McpRequestHeaders::default(),
            McpProtocolVersionPreference::Auto,
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
                &McpRequestHeaders::default(),
                McpProtocolVersionPreference::Auto,
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
                &McpRequestHeaders::default(),
                McpProtocolVersionPreference::Auto,
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
            let Ok((http_method, headers, request)) = read_http_request(&mut stream).await else {
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
