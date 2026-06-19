use std::fmt;

use serde::Serialize;

pub const UPSTREAM_FAILURE_CATEGORY_NETWORK: &str = "network";

pub const UPSTREAM_NETWORK_TIMEOUT: &str = "network.timeout";
pub const UPSTREAM_NETWORK_CONNECT_FAILED: &str = "network.connect_failed";
pub const UPSTREAM_NETWORK_PROXY_FAILED: &str = "network.proxy_failed";
pub const UPSTREAM_NETWORK_DNS_FAILED: &str = "network.dns_failed";
pub const UPSTREAM_NETWORK_TLS_FAILED: &str = "network.tls_failed";
pub const UPSTREAM_NETWORK_BODY_INTERRUPTED: &str = "network.body_interrupted";
pub const UPSTREAM_NETWORK_REQUEST_FAILED: &str = "network.request_failed";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpstreamFailure {
    pub code: String,
    pub category: String,
    pub endpoint: Option<String>,
    pub message_key: String,
}

impl UpstreamFailure {
    pub fn network(
        code: impl Into<String>,
        endpoint: Option<String>,
        message_key: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            category: UPSTREAM_FAILURE_CATEGORY_NETWORK.to_string(),
            endpoint,
            message_key: message_key.into(),
        }
    }

    pub fn fallback_message(&self) -> &'static str {
        match self.code.as_str() {
            UPSTREAM_NETWORK_TIMEOUT => {
                "The request timed out before the target service responded."
            }
            UPSTREAM_NETWORK_CONNECT_FAILED => "Could not connect to the target service.",
            UPSTREAM_NETWORK_PROXY_FAILED => "Could not connect through the configured proxy.",
            UPSTREAM_NETWORK_DNS_FAILED => "Could not find the target service address.",
            UPSTREAM_NETWORK_TLS_FAILED => "Could not establish a secure connection.",
            UPSTREAM_NETWORK_BODY_INTERRUPTED => {
                "The response was interrupted while it was being read."
            }
            UPSTREAM_NETWORK_REQUEST_FAILED => "Network request failed.",
            _ => "Upstream request failed.",
        }
    }
}

impl fmt::Display for UpstreamFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.endpoint.as_deref() {
            Some(endpoint) if !endpoint.is_empty() => {
                write!(formatter, "{} ({endpoint})", self.fallback_message())
            }
            _ => formatter.write_str(self.fallback_message()),
        }
    }
}
