use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;

const PROVIDER_NAME: &str = "Google Vertex AI";

pub(super) async fn list_models(
    _repository: &HttpChatCompletionRepository,
    _config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    Ok(json!({ "bypass": true, "data": [] }))
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let payload_object = payload.as_object().ok_or_else(|| {
        DomainError::InvalidData("Vertex AI payload must be a JSON object".to_string())
    })?;

    let model = payload_object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DomainError::InvalidData("Vertex AI payload missing model".to_string()))?;

    let mut body = payload_object.clone();
    body.remove("model");

    let method = resolve_generation_method(endpoint_path, false);
    let url = HttpChatCompletionRepository::build_url(
        &config.base_url,
        &format!("/publishers/google/models/{model}:{method}"),
    );

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&Value::Object(body));

    let request = if let Some(auth_header) = config.authorization_header.as_deref() {
        HttpChatCompletionRepository::apply_header_if_present(request, "Authorization", auth_header)
    } else {
        request.query(&[("key", config.api_key.as_str())])
    };

    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            PROVIDER_NAME,
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse generation JSON: {error}"))
    })?;

    Ok(normalizers::normalize_gemini_response(body))
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let payload_object = payload.as_object().ok_or_else(|| {
        DomainError::InvalidData("Vertex AI payload must be a JSON object".to_string())
    })?;

    let model = payload_object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DomainError::InvalidData("Vertex AI payload missing model".to_string()))?;

    let mut body = payload_object.clone();
    body.remove("model");

    let method = resolve_generation_method(endpoint_path, true);
    let url = HttpChatCompletionRepository::build_url(
        &config.base_url,
        &format!("/publishers/google/models/{model}:{method}"),
    );

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(&Value::Object(body));

    let request = if let Some(auth_header) = config.authorization_header.as_deref() {
        HttpChatCompletionRepository::apply_header_if_present(request, "Authorization", auth_header)
            .query(&[("alt", "sse")])
    } else {
        request.query(&[("key", config.api_key.as_str()), ("alt", "sse")])
    };

    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            PROVIDER_NAME,
            response,
            "Generation request failed",
        )
        .await);
    }

    HttpChatCompletionRepository::stream_sse_response(PROVIDER_NAME, response, sender, cancel).await
}

fn resolve_generation_method(endpoint_path: &str, stream: bool) -> &'static str {
    let endpoint = endpoint_path.trim().trim_matches('/');

    if endpoint.eq_ignore_ascii_case("streamGenerateContent") {
        return "streamGenerateContent";
    }

    if endpoint.eq_ignore_ascii_case("generateContent") {
        return "generateContent";
    }

    if stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    }
}
