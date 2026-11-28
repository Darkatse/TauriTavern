use std::{collections::HashMap, sync::Arc, time::Duration};

use http::{HeaderName, HeaderValue};
use rmcp::{
    ClientLifecycleMode, RoleClient,
    model::{ClientCapabilities, ClientConfig, Implementation, ProtocolVersion, ServerPeerInfo},
    service::{ClientInitializeError, RunningService, serve_client_with_lifecycle_and_ct},
    transport::{
        common::client_side_sse::NeverRetry,
        streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpClientWorker},
        worker::WorkerTransport,
    },
};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use tt_domain::{
    errors::DomainError,
    models::mcp::{McpEndpoint, McpProtocolVersionPreference, McpRequestHeaders},
};

pub(super) type McpClient = RunningService<RoleClient, ClientConfig>;

const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

struct LifecyclePlan {
    mode: ClientLifecycleMode,
    initialize_version: ProtocolVersion,
    accepted_versions: Vec<ProtocolVersion>,
}

fn lifecycle_plan(preference: McpProtocolVersionPreference) -> LifecyclePlan {
    use McpProtocolVersionPreference::*;

    let versions = [
        (V2026_07_28, ProtocolVersion::V_2026_07_28),
        (V2025_11_25, ProtocolVersion::V_2025_11_25),
        (V2025_06_18, ProtocolVersion::V_2025_06_18),
        (V2025_03_26, ProtocolVersion::V_2025_03_26),
    ];
    if preference == Auto {
        return LifecyclePlan {
            mode: ClientLifecycleMode::Auto {
                preferred_versions: versions
                    .iter()
                    .filter(|(_, version)| !version.has_initialize())
                    .map(|(_, version)| version.clone())
                    .collect(),
                legacy_version: Some(ProtocolVersion::LATEST_WITH_INITIALIZE),
            },
            initialize_version: ProtocolVersion::LATEST_WITH_INITIALIZE,
            accepted_versions: versions.into_iter().map(|(_, version)| version).collect(),
        };
    }

    let (_, version) = versions
        .into_iter()
        .find(|(fixed, _)| *fixed == preference)
        .expect("every fixed MCP preference has a protocol version");
    LifecyclePlan {
        mode: if version.has_initialize() {
            ClientLifecycleMode::Initialize
        } else {
            ClientLifecycleMode::Discover {
                preferred_versions: vec![version.clone()],
            }
        },
        initialize_version: version.clone(),
        accepted_versions: vec![version],
    }
}

fn client_config(initialize_version: ProtocolVersion) -> ClientConfig {
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("TauriTavern", env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(initialize_version)
}

fn transport(
    endpoint: &McpEndpoint,
    request_headers: &HashMap<HeaderName, HeaderValue>,
    http_client: reqwest::Client,
    cancel: CancellationToken,
) -> WorkerTransport<StreamableHttpClientWorker<reqwest::Client>> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint.as_str());
    config.custom_headers = request_headers.clone();
    config.retry_config = Arc::new(NeverRetry::default());
    config.reinit_on_expired_session = false;
    WorkerTransport::spawn_with_ct(StreamableHttpClientWorker::new(http_client, config), cancel)
}

#[derive(Debug, Error)]
pub(super) enum ClientStartupError {
    #[error(transparent)]
    Initialize(#[from] Box<ClientInitializeError>),
    #[error("The server selected MCP {negotiated}; accepted versions: {accepted}")]
    VersionNotAccepted {
        negotiated: ProtocolVersion,
        accepted: String,
    },
    #[error("MCP startup completed without server information")]
    MissingPeerInfo,
}

pub(super) async fn start_client(
    endpoint: &McpEndpoint,
    request_headers: &HashMap<HeaderName, HeaderValue>,
    preference: McpProtocolVersionPreference,
    http_client: reqwest::Client,
    cancel: &CancellationToken,
) -> Result<(McpClient, Arc<ServerPeerInfo>), ClientStartupError> {
    let plan = lifecycle_plan(preference);
    let client_cancel = cancel.child_token();
    // Worker shutdown must close the channel, not masquerade as caller cancellation.
    let transport_cancel = client_cancel.child_token();
    let mut client = serve_client_with_lifecycle_and_ct(
        client_config(plan.initialize_version),
        transport(endpoint, request_headers, http_client, transport_cancel),
        plan.mode,
        client_cancel,
    )
    .await
    .map_err(Box::new)?;

    let error = match client.peer().peer_info() {
        Some(info) => {
            if plan.accepted_versions.contains(&info.protocol_version) {
                return Ok((client, info));
            }
            ClientStartupError::VersionNotAccepted {
                negotiated: info.protocol_version.clone(),
                accepted: plan
                    .accepted_versions
                    .iter()
                    .map(ProtocolVersion::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        }
        None => ClientStartupError::MissingPeerInfo,
    };
    close_client(&mut client).await;
    Err(error)
}

pub(super) async fn close_client(client: &mut McpClient) {
    match client.close_with_timeout(CLOSE_TIMEOUT).await {
        Ok(Some(_)) => {}
        Ok(None) => tracing::warn!("Timed out closing short-lived MCP client"),
        Err(error) => tracing::warn!(%error, "Failed to join short-lived MCP client"),
    }
}

pub(super) fn compile_request_headers(
    headers: &McpRequestHeaders,
) -> Result<HashMap<HeaderName, HeaderValue>, DomainError> {
    headers
        .iter()
        .map(|(name, value)| {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                DomainError::InvalidData(format!(
                    "mcp.header_name_invalid: `{name}` is not a valid HTTP header name"
                ))
            })?;
            let value = HeaderValue::from_bytes(value.as_bytes()).map_err(|_| {
                DomainError::InvalidData(format!(
                    "mcp.header_value_invalid: value for `{name}` is not a valid HTTP header value"
                ))
            })?;
            Ok((name, value))
        })
        .collect()
}
