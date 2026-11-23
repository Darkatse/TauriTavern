use std::borrow::Cow;

use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamDelta,
};

use super::claude;
use super::gemini;
use super::normalizers;
use super::vertexai_auth;
use super::{GenerationRequest, HttpChatCompletionRepository, response_format};
use crate::provider_request::ProviderRequest;
use crate::provider_response::{self, read_upstream_json_body};

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
    let response = send(
        repository,
        config,
        endpoint_path,
        payload,
        false,
        PROVIDER_NAME,
    )
    .await?;

    let body = read_upstream_json_body(PROVIDER_NAME, "generate", response).await?;

    normalizers::normalize_gemini_response(PROVIDER_NAME, body)
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
    let response = send(
        repository,
        config,
        endpoint_path,
        payload,
        false,
        CLAUDE_PROVIDER_NAME,
    )
    .await?;

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
    sender: GenerationStreamSender,
    cancel: GenerationCancelReceiver,
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
    sender: GenerationStreamSender,
    cancel: GenerationCancelReceiver,
) -> Result<(), DomainError> {
    let response = send_gemini_stream_request(repository, config, endpoint_path, payload).await?;

    gemini::stream_generate_content_with_native(PROVIDER_NAME, response, sender, cancel).await
}

async fn send_gemini_stream_request(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<reqwest::Response, DomainError> {
    send(
        repository,
        config,
        endpoint_path,
        payload,
        true,
        PROVIDER_NAME,
    )
    .await
}

async fn generate_claude_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: GenerationStreamSender,
    cancel: GenerationCancelReceiver,
) -> Result<(), DomainError> {
    let (model, response) =
        send_claude_stream_request(repository, config, endpoint_path, payload).await?;

    if super::payload_contains_cache_control(payload) {
        HttpChatCompletionRepository::stream_sse_response_with_cache_logging(
            CLAUDE_PROVIDER_NAME,
            model,
            response,
            sender,
            cancel,
        )
        .await
    } else {
        provider_response::stream_sse_response(CLAUDE_PROVIDER_NAME, response, sender, cancel).await
    }
}

pub(super) async fn generate_with_deltas(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    on_delta: &mut (dyn FnMut(ChatCompletionStreamDelta) + Send),
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    if !is_anthropic_raw_predict_endpoint(endpoint_path) {
        let response =
            send_gemini_stream_request(repository, config, endpoint_path, payload).await?;
        let body =
            gemini::consume_generate_content_stream(PROVIDER_NAME, response, on_delta).await?;
        return normalizers::normalize_gemini_response(PROVIDER_NAME, body);
    }

    let (model, response) =
        send_claude_stream_request(repository, config, endpoint_path, payload).await?;
    let body = claude::consume_message_stream(CLAUDE_PROVIDER_NAME, response, on_delta).await?;

    if super::payload_contains_cache_control(payload) {
        let _ = super::log_prompt_cache_performance_if_present(
            CLAUDE_PROVIDER_NAME,
            Some(model.as_str()),
            &body,
        );
    }

    Ok(normalizers::normalize_claude_response(body))
}

async fn send_claude_stream_request(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<(String, reqwest::Response), DomainError> {
    let model = extract_anthropic_model_id(endpoint_path)
        .map(str::to_string)
        .ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Vertex AI Claude endpoint path is missing model id: {endpoint_path}"
            ))
        })?;
    let response = send(
        repository,
        config,
        endpoint_path,
        payload,
        true,
        CLAUDE_PROVIDER_NAME,
    )
    .await?;

    Ok((model, response))
}

/// Uses a placeholder token; only sending may exchange service-account credentials.
pub(super) fn unsigned_generation_request<'a>(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &'a Value,
    stream: bool,
) -> Result<GenerationRequest<'a>, DomainError> {
    let access_token = service_account(config).map(|_| "access-token");
    generation_request(config, endpoint_path, payload, stream, access_token)
}

/// `access_token` must belong to `service_account(config)`.
fn generation_request<'a>(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &'a Value,
    stream: bool,
    access_token: Option<&str>,
) -> Result<GenerationRequest<'a>, DomainError> {
    let claude = is_anthropic_raw_predict_endpoint(endpoint_path);
    let (path, body) = if claude {
        if !payload.is_object() {
            return Err(DomainError::InvalidData(
                "Vertex AI payload must be a JSON object".to_string(),
            ));
        }
        (
            anthropic_endpoint_path(endpoint_path, stream)?,
            Cow::Borrowed(payload),
        )
    } else {
        let (model, body) = gemini::model_and_body(payload, "Vertex AI")?;
        let method = gemini::generation_method(endpoint_path, stream);
        (
            format!("/publishers/google/models/{model}:{method}"),
            Cow::Owned(body),
        )
    };
    let url = HttpChatCompletionRepository::build_url(&config.base_url, &path)?;

    let mut request = ProviderRequest::post_json(url, response_format(stream));
    if let Some(access_token) = access_token {
        request.header("Authorization", &format!("Bearer {access_token}"))?;
    } else if let Some(authorization_header) = config.authorization_header.as_deref() {
        HttpChatCompletionRepository::apply_header_if_present(
            &mut request,
            "Authorization",
            authorization_header,
        )?;
    } else {
        request.query("key", &config.api_key);
    }
    if stream && !claude {
        request.query("alt", "sse");
    }
    HttpChatCompletionRepository::apply_extra_headers(&mut request, &config.extra_headers)?;
    HttpChatCompletionRepository::apply_additional_headers(&mut request, config)?;
    Ok(GenerationRequest::http(request, body))
}

fn service_account(config: &ChatCompletionApiConfig) -> Option<&str> {
    if config.authorization_header.is_some() {
        return None;
    }
    config.vertexai_service_account_json.as_deref()
}

async fn send(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    stream: bool,
    provider_name: &str,
) -> Result<reqwest::Response, DomainError> {
    let access_token = match service_account(config) {
        Some(service_account_json) => Some(
            vertexai_auth::get_service_account_access_token(
                &repository.http_clients,
                service_account_json,
            )
            .await?,
        ),
        None => None,
    };
    let request = generation_request(
        config,
        endpoint_path,
        payload,
        stream,
        access_token.as_deref(),
    )?;
    repository
        .send_generation(config, request, stream, provider_name)
        .await
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
