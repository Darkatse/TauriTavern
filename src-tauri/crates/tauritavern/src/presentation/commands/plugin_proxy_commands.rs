use std::sync::{Arc, OnceLock};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::app::AppState;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;

/// Reverse proxy for SillyTavern server plugins hosted by an external
/// SillyTavern instance. The frontend route (`routes/plugin-proxy-routes.js`)
/// tunnels `/api/plugins/*` requests here; this command performs the actual
/// HTTP call on the native stack (no WebView CORS restrictions).
///
/// Scope guard: only `/api/plugins/*` paths may be requested so the command
/// can never act as a general-purpose SSRF tunnel for extensions.
const PLUGIN_API_PREFIX: &str = "/api/plugins/";
const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MIN_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESPONSE_BYTES_CEILING: usize = 256 * 1024 * 1024;

/// Hop-by-hop and framing headers that must not be passed through verbatim
/// (the response body is re-framed into the IPC payload).
const STRIPPED_RESPONSE_HEADERS: &[&str] = &[
    "content-length",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "content-encoding",
    "set-cookie",
];

static PROXY_CLIENT: OnceLock<Client> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginProxyRequestDto {
    pub base_url: String,
    pub method: String,
    pub path_and_query: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_response_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginProxyResponseDto {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
}

#[tauri::command]
pub async fn plugin_proxy_request(
    request: PluginProxyRequestDto,
    locale: String,
    app_handle: AppHandle,
    app_state: State<'_, Arc<AppState>>,
) -> Result<PluginProxyResponseDto, CommandError> {
    log_command("plugin_proxy_request");

    let target = validate_target(&request)?;
    ensure_user_endpoint_access(
        Some(request.base_url.trim().trim_end_matches('/').to_string()),
        &locale,
        &app_handle,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;

    execute_proxy(request, target).await
}

fn validate_target(request: &PluginProxyRequestDto) -> Result<String, CommandError> {
    let base = request.base_url.trim().trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(CommandError::BadRequest(
            "Plugin bridge baseUrl must be an http(s) URL".to_string(),
        ));
    }

    let path = request.path_and_query.trim();
    if !path.starts_with(PLUGIN_API_PREFIX) {
        return Err(CommandError::BadRequest(format!(
            "Plugin proxy path must start with {PLUGIN_API_PREFIX}"
        )));
    }

    Ok(format!("{base}{path}"))
}

fn proxy_client() -> Result<&'static Client, CommandError> {
    if let Some(client) = PROXY_CLIENT.get() {
        return Ok(client);
    }

    let client = tt_adapter_http::build_http_client(Client::builder(), crate::product::USER_AGENT)
        .map_err(|error| {
            CommandError::InternalServerError(format!("Failed to build plugin proxy client: {error}"))
        })?;
    Ok(PROXY_CLIENT.get_or_init(|| client))
}

async fn execute_proxy(
    request: PluginProxyRequestDto,
    target: String,
) -> Result<PluginProxyResponseDto, CommandError> {
    let client = proxy_client()?;
    let method = Method::from_bytes(request.method.to_uppercase().as_bytes())
        .map_err(|error| CommandError::BadRequest(format!("Invalid proxy method '{}': {error}", request.method)))?;
    let timeout = Duration::from_millis(
        request
            .timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS),
    );

    let mut builder = client.request(method, &target).timeout(timeout);
    for (name, value) in &request.headers {
        // Request headers are contract input from our own frontend route, so a
        // malformed pair means the contract is broken: fail fast with context
        // instead of silently forwarding a degraded request. Validation stays
        // manual because reqwest's header() panics on invalid names/values.
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| CommandError::BadRequest(format!("Invalid proxy header name '{name}': {error}")))?;
        let header_value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|error| CommandError::BadRequest(format!("Invalid proxy header value for '{name}': {error}")))?;
        builder = builder.header(header_name, header_value);
    }

    if let Some(body) = request.body.as_deref().filter(|body| !body.is_empty()) {
        builder = builder.body(body.to_owned());
    }

    let mut response = match builder.send().await {
        Ok(response) => response,
        // Transport-level failures (connect refused, DNS, timeout) must read
        // as "backend absent" to probe-driven extensions, so surface them as
        // a 502 payload instead of an IPC error.
        Err(error) => {
            return Ok(gateway_error_response(format!("Bridge target unreachable: {error}")))
        }
    };

    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let headers = collect_response_headers(response.headers());
    let max_response_bytes = request
        .max_response_bytes
        .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES)
        .clamp(1, MAX_RESPONSE_BYTES_CEILING);

    let mut body: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > max_response_bytes {
                    return Ok(gateway_error_response(format!(
                        "Plugin response exceeds the {max_response_bytes} byte limit"
                    )));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(error) => {
                return Ok(gateway_error_response(format!("Bridge target read failed: {error}")))
            }
        }
    }

    Ok(PluginProxyResponseDto {
        status: status.as_u16(),
        status_text,
        headers,
        body_base64: STANDARD.encode(body),
    })
}

fn collect_response_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut collected = Vec::new();
    for (name, value) in headers.iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if STRIPPED_RESPONSE_HEADERS.contains(&lower.as_str()) {
            continue;
        }
        // Response headers are upstream input; a non-visible-ASCII value is
        // skipped rather than guessed, but the skip is logged so the caller
        // can see the response was degraded.
        let Ok(value) = value.to_str() else {
            tracing::warn!(header = %name, "Dropping non-UTF-8 plugin response header");
            continue;
        };
        collected.push((lower, value.to_string()));
    }
    collected
}

fn gateway_error_response(message: String) -> PluginProxyResponseDto {
    let body =
        serde_json::json!({ "error": "Plugin bridge unreachable", "message": message }).to_string();
    PluginProxyResponseDto {
        status: 502,
        status_text: "Bad Gateway".to_string(),
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body_base64: STANDARD.encode(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(base_url: &str, path_and_query: &str) -> PluginProxyRequestDto {
        PluginProxyRequestDto {
            base_url: base_url.to_string(),
            method: "GET".to_string(),
            path_and_query: path_and_query.to_string(),
            headers: Vec::new(),
            body: None,
            timeout_ms: None,
            max_response_bytes: None,
        }
    }

    #[test]
    fn validate_target_accepts_plugin_paths_on_http_bases() {
        let target = validate_target(&request(
            "http://127.0.0.1:8000/",
            "/api/plugins/baibaoku/v1/status",
        ))
        .expect("valid target");
        assert_eq!(
            target,
            "http://127.0.0.1:8000/api/plugins/baibaoku/v1/status"
        );
    }

    #[test]
    fn validate_target_rejects_paths_outside_the_plugin_namespace() {
        let error = validate_target(&request(
            "http://127.0.0.1:8000",
            "/api/chats/get",
        ))
        .unwrap_err();
        assert!(matches!(error, CommandError::BadRequest(_)));
    }

    #[test]
    fn validate_target_rejects_non_http_schemes() {
        let error = validate_target(&request(
            "file:///etc/passwd",
            "/api/plugins/baibaoku/v1/status",
        ))
        .unwrap_err();
        assert!(matches!(error, CommandError::BadRequest(_)));
    }
}
