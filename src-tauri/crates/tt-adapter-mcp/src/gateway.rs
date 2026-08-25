use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tt_adapter_http::{HttpClientPool, HttpClientProfile};

use tt_domain::{
    errors::DomainError,
    models::mcp::{McpEndpoint, McpProtocolVersionPreference, McpRequestHeaders},
};
use tt_ports::mcp::{McpCallOutcome, McpDiscoveryResult, McpGateway};

mod client;
mod discovery;
mod tool_call;

#[cfg(test)]
mod tests;

use client::{compile_request_headers, start_client};
use discovery::{list_tools, validate_tools};
use tool_call::{call_tool_with_client, not_sent};

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
        request_headers: &McpRequestHeaders,
        protocol_version: McpProtocolVersionPreference,
    ) -> Result<McpDiscoveryResult, DomainError> {
        let http_client = self.http_clients.client(HttpClientProfile::Mcp)?;
        let request_headers = compile_request_headers(request_headers)?;
        let cancel = CancellationToken::new();
        let mut client = timeout(
            DISCOVERY_TIMEOUT,
            start_client(
                endpoint,
                &request_headers,
                protocol_version,
                http_client,
                &cancel,
            ),
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
        request_headers: &McpRequestHeaders,
        protocol_version: McpProtocolVersionPreference,
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

        let http_client = match self.http_clients.client(HttpClientProfile::Mcp) {
            Ok(client) => client,
            Err(error) => {
                return Ok(not_sent(
                    "mcp.call_http_client_unavailable",
                    format!("Failed to prepare the MCP HTTP client: {error}"),
                ));
            }
        };
        let request_headers = match compile_request_headers(request_headers) {
            Ok(headers) => headers,
            Err(error) => {
                return Ok(not_sent("mcp.call_headers_invalid", error.to_string()));
            }
        };
        let mut client = match timeout(
            DISCOVERY_TIMEOUT,
            start_client(
                endpoint,
                &request_headers,
                protocol_version,
                http_client,
                &cancel,
            ),
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
