use std::{collections::HashMap, sync::Arc};

use http::{HeaderName, HeaderValue};
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
use tt_domain::{
    errors::DomainError,
    models::mcp::{McpEndpoint, McpProtocolVersionPreference, McpRequestHeaders},
};

pub(super) type McpClient = RunningService<RoleClient, ClientInfo>;

fn fixed_protocol_version(preference: McpProtocolVersionPreference) -> Option<ProtocolVersion> {
    match preference {
        McpProtocolVersionPreference::Auto => None,
        McpProtocolVersionPreference::V2026_07_28 => Some(ProtocolVersion::V_2026_07_28),
        McpProtocolVersionPreference::V2025_11_25 => Some(ProtocolVersion::V_2025_11_25),
        McpProtocolVersionPreference::V2025_06_18 => Some(ProtocolVersion::V_2025_06_18),
        McpProtocolVersionPreference::V2025_03_26 => Some(ProtocolVersion::V_2025_03_26),
    }
}

fn legacy_initialize_protocol_version(preference: McpProtocolVersionPreference) -> ProtocolVersion {
    fixed_protocol_version(preference).unwrap_or(ProtocolVersion::V_2025_11_25)
}

fn client_info(protocol_version: ProtocolVersion) -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("TauriTavern", env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(protocol_version)
}

fn auto_lifecycle(preference: McpProtocolVersionPreference) -> ClientLifecycleMode {
    let preferred_versions = match fixed_protocol_version(preference) {
        None => vec![
            ProtocolVersion::V_2026_07_28,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_03_26,
        ],
        Some(version) => vec![version],
    };
    ClientLifecycleMode::Auto {
        preferred_versions,
        legacy_version: Some(legacy_initialize_protocol_version(preference)),
    }
}

fn transport(
    endpoint: &McpEndpoint,
    request_headers: &HashMap<HeaderName, HeaderValue>,
    http_client: reqwest::Client,
    cancel: CancellationToken,
) -> WorkerTransport<StreamableHttpClientWorker<BoundedReqwestClient>> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint.as_str());
    config.custom_headers = request_headers.clone();
    config.retry_config = Arc::new(NeverRetry::default());
    config.max_sse_event_size = MAX_HTTP_RESPONSE_BYTES;
    config.reinit_on_expired_session = false;
    let worker = StreamableHttpClientWorker::new(
        BoundedReqwestClient::new(http_client, MAX_HTTP_RESPONSE_BYTES, cancel.clone()),
        config,
    );
    WorkerTransport::spawn_with_ct(worker, cancel)
}

// RMCP owns this rich protocol error; boxing it here would only move allocation into our adapter.
#[allow(clippy::result_large_err)]
async fn serve_attempt(
    endpoint: &McpEndpoint,
    request_headers: &HashMap<HeaderName, HeaderValue>,
    preference: McpProtocolVersionPreference,
    http_client: reqwest::Client,
    lifecycle: ClientLifecycleMode,
    cancel: &CancellationToken,
) -> Result<McpClient, ClientInitializeError> {
    let attempt_cancel = cancel.child_token();
    // Worker shutdown must close the channel, not masquerade as caller cancellation.
    let transport_cancel = attempt_cancel.child_token();
    serve_client_with_lifecycle_and_ct(
        client_info(legacy_initialize_protocol_version(preference)),
        transport(endpoint, request_headers, http_client, transport_cancel),
        lifecycle,
        attempt_cancel,
    )
    .await
}

#[allow(clippy::result_large_err)]
pub(super) async fn start_client(
    endpoint: &McpEndpoint,
    request_headers: &HashMap<HeaderName, HeaderValue>,
    preference: McpProtocolVersionPreference,
    http_client: reqwest::Client,
    cancel: &CancellationToken,
) -> Result<McpClient, ClientInitializeError> {
    match serve_attempt(
        endpoint,
        request_headers,
        preference,
        http_client.clone(),
        auto_lifecycle(preference),
        cancel,
    )
    .await
    {
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
                request_headers,
                preference,
                http_client,
                ClientLifecycleMode::Initialize,
                cancel,
            )
            .await
        }
        result => result,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_preference_keeps_auto_and_fixed_semantics_distinct() {
        let auto = McpProtocolVersionPreference::Auto;
        assert_eq!(
            legacy_initialize_protocol_version(auto),
            ProtocolVersion::V_2025_11_25
        );
        assert!(matches!(
            auto_lifecycle(auto),
            ClientLifecycleMode::Auto {
                preferred_versions,
                legacy_version: Some(legacy_version),
            } if preferred_versions[0] == ProtocolVersion::V_2026_07_28
                && legacy_version == ProtocolVersion::V_2025_11_25
        ));

        let fixed = McpProtocolVersionPreference::V2025_06_18;
        assert_eq!(
            legacy_initialize_protocol_version(fixed),
            ProtocolVersion::V_2025_06_18
        );
        assert!(matches!(
            auto_lifecycle(fixed),
            ClientLifecycleMode::Auto {
                preferred_versions,
                legacy_version: Some(legacy_version),
            } if preferred_versions == [ProtocolVersion::V_2025_06_18]
                && legacy_version == ProtocolVersion::V_2025_06_18
        ));
    }

    #[test]
    fn request_headers_are_compiled_by_the_http_adapter() {
        let headers = McpRequestHeaders::from(std::collections::BTreeMap::from([(
            "x-label".to_string(),
            "用户选择".to_string(),
        )]));
        assert_eq!(
            compile_request_headers(&headers)
                .unwrap()
                .get(&HeaderName::from_static("x-label"))
                .unwrap()
                .as_bytes(),
            "用户选择".as_bytes()
        );

        for headers in [
            std::collections::BTreeMap::from([("invalid name".to_string(), "value".to_string())]),
            std::collections::BTreeMap::from([("x-label".to_string(), "line\nbreak".to_string())]),
        ] {
            assert!(compile_request_headers(&McpRequestHeaders::from(headers)).is_err());
        }
    }
}
