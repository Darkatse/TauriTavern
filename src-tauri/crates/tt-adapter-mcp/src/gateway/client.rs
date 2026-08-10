use std::sync::Arc;

use rmcp::{
    ClientLifecycleMode, RoleClient,
    model::{ClientCapabilities, ClientInfo, Implementation, ProtocolVersion},
    service::{ClientInitializeError, RunningService, serve_client_with_lifecycle_and_ct},
    transport::{
        common::client_side_sse::NeverRetry,
        streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpClientWorker},
        worker::WorkerTransport,
    },
};
use tokio_util::sync::CancellationToken;

use crate::bounded_http_client::{BoundedReqwestClient, MAX_HTTP_RESPONSE_BYTES};
use tt_domain::models::mcp::McpEndpoint;

pub(super) type McpClient = RunningService<RoleClient, ClientInfo>;

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

pub(super) async fn start_client(
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
