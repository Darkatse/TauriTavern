use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    provider_name: &str,
) -> Result<Value, DomainError> {
    list_models_with_path(repository, config, provider_name, "/models").await
}

pub(super) async fn list_models_with_path(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    provider_name: &str,
    path: &str,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, path);

    let client = repository.client()?;
    let request = client.get(url).header(ACCEPT, "application/json");
    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request
        .send()
        .await
        .map_err(|error| DomainError::InternalError(format!("Status request failed: {error}")))?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
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
    provider_name: &str,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse generation JSON: {error}"))
    })?;

    if super::payload_contains_cache_control(payload) {
        let model = payload.get("model").and_then(Value::as_str);
        let _ = super::log_prompt_cache_performance_if_present(provider_name, model, &body);
    }

    Ok(body)
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
            response,
            "Generation request failed",
        )
        .await);
    }

    if super::payload_contains_cache_control(payload) {
        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut logged = false;

        HttpChatCompletionRepository::stream_sse_response_internal(
            provider_name,
            response,
            sender,
            cancel,
            move |payload| {
                if logged {
                    return;
                }

                if !payload.windows(b"cache_read_input_tokens".len()).any(|window| {
                    window == b"cache_read_input_tokens"
                }) && !payload
                    .windows(b"cache_creation_input_tokens".len())
                    .any(|window| window == b"cache_creation_input_tokens")
                {
                    return;
                }

                let Ok(value) = serde_json::from_slice::<Value>(payload) else {
                    return;
                };

                logged = super::log_prompt_cache_performance_if_present(
                    provider_name,
                    Some(model.as_str()),
                    &value,
                );
            },
        )
        .await
    } else {
        HttpChatCompletionRepository::stream_sse_response(provider_name, response, sender, cancel)
            .await
    }
}
