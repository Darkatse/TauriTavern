use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;

const GEMINI_API_VERSION: &str = "v1beta";

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let url = build_gemini_url(&config.base_url, "models");

    let client = repository.client()?;
    let request = client.get(url).header(ACCEPT, "application/json");
    let request = apply_gemini_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request
        .send()
        .await
        .map_err(|error| DomainError::InternalError(format!("Status request failed: {error}")))?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Google Gemini",
            response,
            "Failed to list models",
        )
        .await);
    }

    let body = response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse models JSON: {error}"))
    })?;

    let models = body
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter(|model| {
                    model
                        .get("supportedGenerationMethods")
                        .and_then(Value::as_array)
                        .is_some_and(|methods| {
                            methods
                                .iter()
                                .any(|entry| entry.as_str() == Some("generateContent"))
                        })
                })
                .filter_map(|model| {
                    let id = model
                        .get("name")
                        .and_then(Value::as_str)
                        .map(|name| name.trim_start_matches("models/"))
                        .map(str::trim)
                        .filter(|name| !name.is_empty())?;

                    Some(json!({ "id": id }))
                })
                .collect::<Vec<Value>>()
        })
        .unwrap_or_default();

    Ok(json!({ "data": models }))
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let payload_object = payload.as_object().ok_or_else(|| {
        DomainError::InvalidData("Gemini payload must be a JSON object".to_string())
    })?;

    let model = payload_object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DomainError::InvalidData("Gemini payload missing model".to_string()))?;

    let mut body = payload_object.clone();
    body.remove("model");

    let method = resolve_generation_method(endpoint_path, false);
    let model_path = format!("{}:{method}", normalize_gemini_model(model));
    let url = build_gemini_url(&config.base_url, &model_path);

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&Value::Object(body));

    let request = apply_gemini_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Google Gemini",
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
        DomainError::InvalidData("Gemini payload must be a JSON object".to_string())
    })?;

    let model = payload_object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DomainError::InvalidData("Gemini payload missing model".to_string()))?;

    let mut body = payload_object.clone();
    body.remove("model");

    let method = resolve_generation_method(endpoint_path, true);
    let model_path = format!("{}:{method}", normalize_gemini_model(model));
    let url = build_gemini_url(&config.base_url, &model_path);

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(&Value::Object(body));

    let request = apply_gemini_stream_auth(request, config);

    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Google Gemini",
            response,
            "Generation request failed",
        )
        .await);
    }

    HttpChatCompletionRepository::stream_sse_response("Google Gemini", response, sender, cancel)
        .await
}

fn normalize_gemini_model(model: &str) -> String {
    let model = model.trim();
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}

fn apply_gemini_auth(
    request: reqwest::RequestBuilder,
    config: &ChatCompletionApiConfig,
) -> reqwest::RequestBuilder {
    if let Some(authorization_header) = config.authorization_header.as_deref() {
        return HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            authorization_header,
        );
    }

    let request = HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-goog-api-key",
        &config.api_key,
    );

    if config.api_key.trim().is_empty() {
        request
    } else {
        request.query(&[("key", config.api_key.as_str())])
    }
}

fn apply_gemini_stream_auth(
    request: reqwest::RequestBuilder,
    config: &ChatCompletionApiConfig,
) -> reqwest::RequestBuilder {
    if let Some(authorization_header) = config.authorization_header.as_deref() {
        let request = HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            authorization_header,
        );
        return request.query(&[("alt", "sse")]);
    }

    let request = HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-goog-api-key",
        &config.api_key,
    );

    if config.api_key.trim().is_empty() {
        request.query(&[("alt", "sse")])
    } else {
        request.query(&[("key", config.api_key.as_str()), ("alt", "sse")])
    }
}

fn build_gemini_url(base_url: &str, suffix: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');

    if trimmed.ends_with("/v1") || trimmed.ends_with("/v1beta") {
        format!("{trimmed}/{suffix}")
    } else {
        format!("{trimmed}/{GEMINI_API_VERSION}/{suffix}")
    }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reqwest::Client;
    use reqwest::header::{AUTHORIZATION, HeaderName};

    use super::apply_gemini_auth;
    use crate::domain::repositories::chat_completion_repository::{
        AnthropicBetaHeaderMode, ChatCompletionApiConfig,
    };

    #[test]
    fn gemini_auth_prefers_explicit_authorization_header() {
        let config = ChatCompletionApiConfig {
            base_url: "https://example.com".to_string(),
            api_key: "saved-secret".to_string(),
            authorization_header: Some("Bearer override".to_string()),
            extra_headers: HashMap::new(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
        };

        let request = Client::new().get("https://example.com");
        let request = apply_gemini_auth(request, &config)
            .build()
            .expect("request should build");

        assert_eq!(
            request
                .headers()
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer override")
        );
        assert!(
            request
                .headers()
                .get(HeaderName::from_static("x-goog-api-key"))
                .is_none()
        );
        assert_eq!(request.url().query(), None);
    }
}
