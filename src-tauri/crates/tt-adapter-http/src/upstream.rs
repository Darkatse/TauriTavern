//! Transport failures of outbound requests, classified and logged once for every caller.

use std::error::Error;

use bytes::Bytes;
use reqwest::{Response, Url};
use tt_domain::models::upstream_failure::{
    UPSTREAM_NETWORK_BODY_INTERRUPTED, UPSTREAM_NETWORK_CONNECT_FAILED,
    UPSTREAM_NETWORK_DNS_FAILED, UPSTREAM_NETWORK_PROXY_FAILED, UPSTREAM_NETWORK_REDIRECT_REJECTED,
    UPSTREAM_NETWORK_REQUEST_FAILED, UPSTREAM_NETWORK_TIMEOUT, UPSTREAM_NETWORK_TLS_FAILED,
    UpstreamFailure,
};

pub fn request_failure(error: reqwest::Error) -> UpstreamFailure {
    let endpoint = error.url().map(sanitize_endpoint);
    classify(&error, endpoint, None)
}

pub fn body_failure(error: reqwest::Error, endpoint: &Url) -> UpstreamFailure {
    let endpoint = Some(sanitize_endpoint(error.url().unwrap_or(endpoint)));
    classify(&error, endpoint, Some(UPSTREAM_NETWORK_BODY_INTERRUPTED))
}

pub async fn read_body(response: Response) -> Result<Bytes, UpstreamFailure> {
    let endpoint = response.url().clone();
    response
        .bytes()
        .await
        .map_err(|error| body_failure(error, &endpoint))
}

pub fn sanitize_endpoint(url: &Url) -> String {
    let mut sanitized = url.clone();
    let _ = sanitized.set_username("");
    let _ = sanitized.set_password(None);
    sanitized.set_query(None);
    sanitized.set_fragment(None);
    sanitized.to_string()
}

fn classify(
    error: &reqwest::Error,
    endpoint: Option<String>,
    preferred_code: Option<&str>,
) -> UpstreamFailure {
    let cause = source_chain_text(error);
    let signals = cause.to_ascii_lowercase();
    let code = if error.is_timeout() {
        UPSTREAM_NETWORK_TIMEOUT
    } else if error.is_connect() && contains_proxy_signal(&signals) {
        UPSTREAM_NETWORK_PROXY_FAILED
    } else if error.is_connect() && contains_dns_signal(&signals) {
        UPSTREAM_NETWORK_DNS_FAILED
    } else if error.is_connect() && contains_tls_signal(&signals) {
        UPSTREAM_NETWORK_TLS_FAILED
    } else if error.is_connect() {
        UPSTREAM_NETWORK_CONNECT_FAILED
    } else if error.is_redirect() {
        UPSTREAM_NETWORK_REDIRECT_REJECTED
    } else if let Some(code) = preferred_code {
        code
    } else if error.is_body() {
        UPSTREAM_NETWORK_BODY_INTERRUPTED
    } else {
        UPSTREAM_NETWORK_REQUEST_FAILED
    };

    tracing::warn!(
        code,
        endpoint = endpoint.as_deref().unwrap_or(""),
        cause = %cause,
        "upstream transport failed",
    );
    UpstreamFailure::network(code, endpoint, message_key_for_code(code))
}

fn message_key_for_code(code: &str) -> &'static str {
    match code {
        UPSTREAM_NETWORK_TIMEOUT => "tauritavern.error.network.timeout",
        UPSTREAM_NETWORK_CONNECT_FAILED => "tauritavern.error.network.connect_failed",
        UPSTREAM_NETWORK_PROXY_FAILED => "tauritavern.error.network.proxy_failed",
        UPSTREAM_NETWORK_DNS_FAILED => "tauritavern.error.network.dns_failed",
        UPSTREAM_NETWORK_TLS_FAILED => "tauritavern.error.network.tls_failed",
        UPSTREAM_NETWORK_BODY_INTERRUPTED => "tauritavern.error.network.body_interrupted",
        UPSTREAM_NETWORK_REDIRECT_REJECTED => "tauritavern.error.network.redirect_rejected",
        _ => "tauritavern.error.network.request_failed",
    }
}

// Skips the top-level reqwest message, which carries the full URL with its query.
fn source_chain_text(error: &(dyn Error + 'static)) -> String {
    let mut parts = Vec::new();
    let mut current = error.source();
    while let Some(source) = current {
        parts.push(source.to_string());
        current = source.source();
    }
    parts.join(": ")
}

fn contains_proxy_signal(text: &str) -> bool {
    text.contains("proxy") || text.contains("socks")
}

fn contains_dns_signal(text: &str) -> bool {
    text.contains("dns")
        || text.contains("resolve")
        || text.contains("lookup")
        || text.contains("name or service not known")
        || text.contains("nodename")
}

fn contains_tls_signal(text: &str) -> bool {
    text.contains("tls")
        || text.contains("certificate")
        || text.contains("cert")
        || text.contains("rustls")
        || text.contains("invalid peer")
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt;

    use reqwest::Url;

    use super::{sanitize_endpoint, source_chain_text};

    #[test]
    fn sanitize_endpoint_removes_query_fragment_and_userinfo() {
        let url = Url::parse("https://user:secret@example.com/v1/models?key=abc#frag").unwrap();

        assert_eq!(sanitize_endpoint(&url), "https://example.com/v1/models");
    }

    #[derive(Debug)]
    struct TopError;

    impl fmt::Display for TopError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("error sending request for url (https://proxy.example.test)")
        }
    }

    impl Error for TopError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&SOURCE_ERROR)
        }
    }

    #[derive(Debug)]
    struct SourceError;

    impl fmt::Display for SourceError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("connection refused")
        }
    }

    impl Error for SourceError {}

    static SOURCE_ERROR: SourceError = SourceError;

    #[test]
    fn source_chain_text_excludes_top_level_reqwest_display() {
        assert_eq!(source_chain_text(&TopError), "connection refused");
    }
}
