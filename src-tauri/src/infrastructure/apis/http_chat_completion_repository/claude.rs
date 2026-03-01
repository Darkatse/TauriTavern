use std::collections::HashMap;

use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_BETA_OUTPUT_128K: &str = "output-128k-2025-02-19";
const ANTHROPIC_BETA_CONTEXT_1M: &str = "context-1m-2025-08-07";
const ANTHROPIC_BETA_PROMPT_CACHING: &str = "prompt-caching-2024-07-31";
const ANTHROPIC_BETA_EXTENDED_CACHE_TTL: &str = "extended-cache-ttl-2025-04-11";

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, "/models");

    let request = repository
        .client
        .get(url)
        .header(ACCEPT, "application/json")
        .header("anthropic-version", ANTHROPIC_VERSION);

    let request = HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-api-key",
        &config.api_key,
    );
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request
        .send()
        .await
        .map_err(|error| DomainError::InternalError(format!("Status request failed: {error}")))?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Claude",
            response,
            "Failed to list models",
        )
        .await);
    }

    response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse models JSON: {error}"))
    })
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<Value, DomainError> {
    let endpoint_path = if endpoint_path.trim().is_empty() {
        "/messages"
    } else {
        endpoint_path
    };

    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let request = repository
        .client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(payload);

    let request = HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-api-key",
        &config.api_key,
    );
    let request = apply_anthropic_beta_header(request, config, payload);
    let request = HttpChatCompletionRepository::apply_extra_headers_with_filter(
        request,
        &config.extra_headers,
        |key, _| key.eq_ignore_ascii_case("anthropic-beta"),
    );

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Claude",
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse generation JSON: {error}"))
    })?;

    Ok(normalizers::normalize_claude_response(body))
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let endpoint_path = if endpoint_path.trim().is_empty() {
        "/messages"
    } else {
        endpoint_path
    };

    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let request = repository
        .stream_client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(payload);

    let request = HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-api-key",
        &config.api_key,
    );
    let request = apply_anthropic_beta_header(request, config, payload);
    let request = HttpChatCompletionRepository::apply_extra_headers_with_filter(
        request,
        &config.extra_headers,
        |key, _| key.eq_ignore_ascii_case("anthropic-beta"),
    );

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Claude",
            response,
            "Generation request failed",
        )
        .await);
    }

    HttpChatCompletionRepository::stream_sse_response("Claude", response, sender, cancel).await
}

fn apply_anthropic_beta_header(
    request: RequestBuilder,
    config: &ChatCompletionApiConfig,
    payload: &Value,
) -> RequestBuilder {
    let beta_values = build_anthropic_beta_values(&config.extra_headers, payload);

    if beta_values.is_empty() {
        return request;
    }

    request.header("anthropic-beta", beta_values.join(","))
}

fn build_anthropic_beta_values(
    extra_headers: &HashMap<String, String>,
    payload: &Value,
) -> Vec<String> {
    let mut beta_values = vec![
        ANTHROPIC_BETA_OUTPUT_128K.to_string(),
        ANTHROPIC_BETA_CONTEXT_1M.to_string(),
    ];

    for value in configured_anthropic_beta_values(extra_headers) {
        if !beta_values.iter().any(|existing| existing == &value) {
            beta_values.push(value);
        }
    }

    if payload_contains_cache_control(payload) {
        for value in [
            ANTHROPIC_BETA_PROMPT_CACHING,
            ANTHROPIC_BETA_EXTENDED_CACHE_TTL,
        ] {
            if !beta_values.iter().any(|existing| existing == value) {
                beta_values.push(value.to_string());
            }
        }
    }

    beta_values
}

fn configured_anthropic_beta_values(extra_headers: &HashMap<String, String>) -> Vec<String> {
    let Some(raw_value) = extra_headers
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case("anthropic-beta").then_some(value))
    else {
        return Vec::new();
    };

    raw_value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn payload_contains_cache_control(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("cache_control")
                || object.values().any(payload_contains_cache_control)
        }
        Value::Array(array) => array.iter().any(payload_contains_cache_control),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{
        ANTHROPIC_BETA_CONTEXT_1M, ANTHROPIC_BETA_EXTENDED_CACHE_TTL, ANTHROPIC_BETA_OUTPUT_128K,
        ANTHROPIC_BETA_PROMPT_CACHING, build_anthropic_beta_values,
        configured_anthropic_beta_values, payload_contains_cache_control,
    };

    #[test]
    fn detects_cache_control_recursively() {
        let payload = json!({
            "messages": [{
                "content": [{
                    "type": "text",
                    "cache_control": { "type": "ephemeral", "ttl": "5m" }
                }]
            }]
        });

        assert!(payload_contains_cache_control(&payload));
    }

    #[test]
    fn parses_existing_beta_header_values() {
        let mut headers = HashMap::new();
        headers.insert(
            "anthropic-beta".to_string(),
            format!(
                "  {}, {}  ",
                ANTHROPIC_BETA_PROMPT_CACHING, ANTHROPIC_BETA_EXTENDED_CACHE_TTL
            ),
        );

        let parsed = configured_anthropic_beta_values(&headers);
        assert_eq!(
            parsed,
            vec![
                ANTHROPIC_BETA_PROMPT_CACHING.to_string(),
                ANTHROPIC_BETA_EXTENDED_CACHE_TTL.to_string()
            ]
        );
    }

    #[test]
    fn always_includes_default_beta_values() {
        let headers = HashMap::new();
        let payload = json!({ "messages": [{"role": "user", "content": "hello"}] });

        let beta_values = build_anthropic_beta_values(&headers, &payload);
        assert!(beta_values.contains(&ANTHROPIC_BETA_OUTPUT_128K.to_string()));
        assert!(beta_values.contains(&ANTHROPIC_BETA_CONTEXT_1M.to_string()));
    }

    #[test]
    fn cache_control_adds_cache_beta_values() {
        let headers = HashMap::new();
        let payload = json!({
            "messages": [{
                "content": [{
                    "type": "text",
                    "cache_control": { "type": "ephemeral", "ttl": "5m" }
                }]
            }]
        });

        let beta_values = build_anthropic_beta_values(&headers, &payload);
        assert!(beta_values.contains(&ANTHROPIC_BETA_PROMPT_CACHING.to_string()));
        assert!(beta_values.contains(&ANTHROPIC_BETA_EXTENDED_CACHE_TTL.to_string()));
    }
}
