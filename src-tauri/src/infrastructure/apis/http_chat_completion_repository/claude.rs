use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionApiConfig;

use super::normalizers;
use super::HttpChatCompletionRepository;

const ANTHROPIC_VERSION: &str = "2023-06-01";

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
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

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
