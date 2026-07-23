use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Map, Value, json};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;
use super::response_body::read_upstream_json_body;
use super::vertexai_auth;

const PROVIDER_NAME: &str = "Google Vertex AI";
const CLAUDE_PROVIDER_NAME: &str = "Google Vertex AI Claude";

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
    if is_anthropic_raw_predict_endpoint(endpoint_path) {
        return generate_claude(repository, config, endpoint_path, payload).await;
    }

    generate_gemini(repository, config, endpoint_path, payload).await
}

async fn generate_gemini(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let (model, body) = extract_model_and_body(payload)?;

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

    let request = apply_vertexai_auth(request, config).await?;

    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            PROVIDER_NAME,
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = read_upstream_json_body(PROVIDER_NAME, "generate", response).await?;

    Ok(normalizers::normalize_gemini_response(body))
}

async fn generate_claude(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let model = extract_anthropic_model_id(endpoint_path).ok_or_else(|| {
        DomainError::InvalidData(format!(
            "Vertex AI Claude endpoint path is missing model id: {endpoint_path}"
        ))
    })?;
    let body = payload_object(payload)?;
    let endpoint_path = anthropic_endpoint_path(endpoint_path, false)?;
    let url = HttpChatCompletionRepository::build_url(&config.base_url, &endpoint_path);

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&Value::Object(body));

    let request = apply_vertexai_auth(request, config).await?;
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            CLAUDE_PROVIDER_NAME,
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = read_upstream_json_body(CLAUDE_PROVIDER_NAME, "generate", response).await?;

    if super::payload_contains_cache_control(payload) {
        let _ = super::log_prompt_cache_performance_if_present(
            CLAUDE_PROVIDER_NAME,
            Some(model),
            &body,
        );
    }

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
    if is_anthropic_raw_predict_endpoint(endpoint_path) {
        return generate_claude_stream(repository, config, endpoint_path, payload, sender, cancel)
            .await;
    }

    generate_gemini_stream(repository, config, endpoint_path, payload, sender, cancel).await
}

async fn generate_gemini_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let (model, body) = extract_model_and_body(payload)?;

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

    let request = apply_vertexai_auth(request, config)
        .await?
        .query(&[("alt", "sse")]);

    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
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

async fn generate_claude_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let model = extract_anthropic_model_id(endpoint_path).ok_or_else(|| {
        DomainError::InvalidData(format!(
            "Vertex AI Claude endpoint path is missing model id: {endpoint_path}"
        ))
    })?;
    let body = payload_object(payload)?;
    let endpoint_path = anthropic_endpoint_path(endpoint_path, true)?;
    let url = HttpChatCompletionRepository::build_url(&config.base_url, &endpoint_path);

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(&Value::Object(body));

    let request = apply_vertexai_auth(request, config).await?;
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            CLAUDE_PROVIDER_NAME,
            response,
            "Generation request failed",
        )
        .await);
    }

    if super::payload_contains_cache_control(payload) {
        let mut logged = false;
        HttpChatCompletionRepository::stream_sse_response_internal(
            CLAUDE_PROVIDER_NAME,
            response,
            sender,
            cancel,
            move |payload| {
                if logged {
                    return Ok(());
                }

                if !payload
                    .windows(b"cache_read_input_tokens".len())
                    .any(|window| window == b"cache_read_input_tokens")
                    && !payload
                        .windows(b"cache_creation_input_tokens".len())
                        .any(|window| window == b"cache_creation_input_tokens")
                {
                    return Ok(());
                }

                let Ok(value) = serde_json::from_slice::<Value>(payload) else {
                    return Ok(());
                };

                logged = super::log_prompt_cache_performance_if_present(
                    CLAUDE_PROVIDER_NAME,
                    Some(model),
                    &value,
                );
                Ok(())
            },
        )
        .await
    } else {
        HttpChatCompletionRepository::stream_sse_response(
            CLAUDE_PROVIDER_NAME,
            response,
            sender,
            cancel,
        )
        .await
    }
}

fn extract_model_and_body(payload: &Value) -> Result<(String, Map<String, Value>), DomainError> {
    let payload_object = payload.as_object().ok_or_else(|| {
        DomainError::InvalidData("Vertex AI payload must be a JSON object".to_string())
    })?;

    let model = payload_object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DomainError::InvalidData("Vertex AI payload missing model".to_string()))?
        .to_string();

    let mut body = payload_object.clone();
    body.remove("model");

    Ok((model, body))
}

fn payload_object(payload: &Value) -> Result<Map<String, Value>, DomainError> {
    payload.as_object().cloned().ok_or_else(|| {
        DomainError::InvalidData("Vertex AI payload must be a JSON object".to_string())
    })
}

fn is_anthropic_raw_predict_endpoint(endpoint_path: &str) -> bool {
    extract_anthropic_model_id(endpoint_path).is_some()
}

fn extract_anthropic_model_id(endpoint_path: &str) -> Option<&str> {
    let endpoint = endpoint_path.trim().trim_matches('/');
    let rest = endpoint.strip_prefix("publishers/anthropic/models/")?;
    let (model, method) = rest.rsplit_once(':')?;
    let method = method.to_ascii_lowercase();
    if model.is_empty() || (method != "rawpredict" && method != "streamrawpredict") {
        return None;
    }

    Some(model)
}

fn anthropic_endpoint_path(endpoint_path: &str, stream: bool) -> Result<String, DomainError> {
    let endpoint_path = endpoint_path.trim();
    let (base, _) = endpoint_path.rsplit_once(':').ok_or_else(|| {
        DomainError::InvalidData(format!(
            "Vertex AI Claude endpoint path is missing rawPredict method: {endpoint_path}"
        ))
    })?;

    let method = if stream {
        "streamRawPredict"
    } else {
        "rawPredict"
    };

    Ok(format!("{base}:{method}"))
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

async fn apply_vertexai_auth(
    request: reqwest::RequestBuilder,
    config: &ChatCompletionApiConfig,
) -> Result<reqwest::RequestBuilder, DomainError> {
    if let Some(auth_header) = config.authorization_header.as_deref() {
        return Ok(HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            auth_header,
        ));
    }

    if let Some(service_account_json) = config.vertexai_service_account_json.as_deref() {
        let access_token =
            vertexai_auth::get_service_account_access_token(service_account_json).await?;
        let auth_header = format!("Bearer {access_token}");
        return Ok(HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            &auth_header,
        ));
    }

    Ok(request.query(&[("key", config.api_key.as_str())]))
}

#[cfg(test)]
mod tests {
    use super::{
        anthropic_endpoint_path, extract_anthropic_model_id, is_anthropic_raw_predict_endpoint,
    };

    #[test]
    fn raw_predict_endpoint_selects_anthropic_vertex_transport() {
        let endpoint = "/publishers/anthropic/models/claude-sonnet-4-5@20250929:rawPredict";
        assert!(is_anthropic_raw_predict_endpoint(endpoint));
        assert_eq!(
            extract_anthropic_model_id(endpoint),
            Some("claude-sonnet-4-5@20250929")
        );
        assert!(!is_anthropic_raw_predict_endpoint("/generateContent"));
    }

    #[test]
    fn stream_requests_upgrade_raw_predict_to_stream_raw_predict() {
        let endpoint = "/publishers/anthropic/models/claude-sonnet-4-5@20250929:rawPredict";
        assert_eq!(
            anthropic_endpoint_path(endpoint, false).unwrap(),
            "/publishers/anthropic/models/claude-sonnet-4-5@20250929:rawPredict"
        );
        assert_eq!(
            anthropic_endpoint_path(endpoint, true).unwrap(),
            "/publishers/anthropic/models/claude-sonnet-4-5@20250929:streamRawPredict"
        );
    }
}
