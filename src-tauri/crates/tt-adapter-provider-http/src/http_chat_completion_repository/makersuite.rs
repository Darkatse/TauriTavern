use std::borrow::Cow;
use std::collections::HashSet;

use reqwest::Url;
use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamDelta,
};

use super::gemini;
use super::normalizers;
use super::{GenerationRequest, HttpChatCompletionRepository, response_format};
use crate::endpoint_url::append_google_api_path;
use crate::provider_request::ProviderRequest;
use crate::provider_response::{self, read_upstream_json_body};

const GEMINI_API_VERSION: &str = "v1beta";

#[derive(serde::Deserialize)]
struct ModelsPage {
    // Proxies such as OpenCode serve another format's list here: no models, not a failed connection.
    #[serde(default)]
    models: Vec<Value>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let url = build_gemini_url(&config.base_url, "models")?;
    let client = repository.metadata_client(config)?;
    let mut page_token: Option<String> = None;
    let mut requested_tokens = HashSet::new();
    let mut models = Vec::new();

    loop {
        let mut request = ProviderRequest::get_json(url.clone());
        request.query("pageSize", "1000");
        if let Some(token) = &page_token {
            request.query("pageToken", token);
        }
        apply_headers(&mut request, config)?;
        let response = provider_response::send_checked(
            request.with_client(&client),
            "Google Gemini",
            "Failed to list models",
        )
        .await?;
        let body = read_upstream_json_body("Google Gemini", "list_models", response).await?;
        let page: ModelsPage = serde_json::from_value(body).map_err(|error| {
            DomainError::InvalidData(format!("Invalid Google Gemini models page: {error}"))
        })?;

        let next = page.next_page_token.filter(|token| !token.is_empty());
        if next
            .as_ref()
            .is_some_and(|token| requested_tokens.contains(token))
        {
            tracing::warn!(
                "Google Gemini models pagination stopped: the server repeated a page token"
            );
            break;
        }

        for mut model in page.models {
            if !model
                .get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_some_and(|methods| methods.iter().any(|method| method == "generateContent"))
            {
                continue;
            }
            let name = model
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    DomainError::InvalidData("Google Gemini model is missing name".to_string())
                })?;
            model["id"] = Value::String(name.strip_prefix("models/").unwrap_or(name).to_string());
            models.push(model);
        }

        let Some(next) = next else { break };
        requested_tokens.insert(next.clone());
        page_token = Some(next);
    }

    Ok(json!({ "data": models }))
}

pub(super) fn generation_request<'a>(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &'a Value,
    stream: bool,
) -> Result<GenerationRequest<'a>, DomainError> {
    let (model, body) = gemini::model_and_body(payload, "Gemini")?;
    let method = gemini::generation_method(endpoint_path, stream);
    let model_path = format!("{}:{method}", normalize_gemini_model(model));
    let url = build_gemini_url(&config.base_url, &model_path)?;

    let mut request = ProviderRequest::post_json(url, response_format(stream));
    apply_headers(&mut request, config)?;
    if stream {
        request.query("alt", "sse");
    }
    Ok(GenerationRequest::http(request, Cow::Owned(body)))
}

async fn send(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    stream: bool,
) -> Result<reqwest::Response, DomainError> {
    let request = generation_request(config, endpoint_path, payload, stream)?;
    repository
        .send_generation(config, request, stream, "Google Gemini")
        .await
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let response = send(repository, config, endpoint_path, payload, false).await?;
    let body = read_upstream_json_body("Google Gemini", "generate", response).await?;

    normalizers::normalize_gemini_response("Google Gemini", body)
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: GenerationStreamSender,
    cancel: GenerationCancelReceiver,
) -> Result<(), DomainError> {
    let response = send(repository, config, endpoint_path, payload, true).await?;

    gemini::stream_generate_content_with_native("Google Gemini", response, sender, cancel).await
}

pub(super) async fn generate_with_deltas(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    on_delta: &mut (dyn FnMut(ChatCompletionStreamDelta) + Send),
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let response = send(repository, config, endpoint_path, payload, true).await?;
    let body = gemini::consume_generate_content_stream("Google Gemini", response, on_delta).await?;

    normalizers::normalize_gemini_response("Google Gemini", body)
}

fn normalize_gemini_model(model: &str) -> String {
    let model = model.trim();
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}

fn apply_headers(
    request: &mut ProviderRequest,
    config: &ChatCompletionApiConfig,
) -> Result<(), DomainError> {
    apply_gemini_auth(request, config)?;
    HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers)?;
    HttpChatCompletionRepository::apply_additional_headers(request, config)
}

/// An API key goes out both as a header and as the `key` query parameter.
fn apply_gemini_auth(
    request: &mut ProviderRequest,
    config: &ChatCompletionApiConfig,
) -> Result<(), DomainError> {
    if let Some(authorization_header) = config.authorization_header.as_deref() {
        return HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            authorization_header,
        );
    }

    if !config.api_key.trim().is_empty() {
        request.header("x-goog-api-key", &config.api_key)?;
        request.query("key", &config.api_key);
    }
    Ok(())
}

fn build_gemini_url(base_url: &str, suffix: &str) -> Result<Url, DomainError> {
    append_google_api_path(base_url, GEMINI_API_VERSION, suffix)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reqwest::Url;

    use super::apply_gemini_auth;
    use crate::provider_request::ProviderRequest;
    use tt_ports::repositories::chat_completion_repository::{
        AnthropicBetaHeaderMode, ChatCompletionApiConfig,
    };

    #[test]
    fn gemini_auth_prefers_explicit_authorization_header() {
        let config = ChatCompletionApiConfig {
            base_url: "https://example.com".to_string(),
            user_configured_endpoint: false,
            api_key: "saved-secret".to_string(),
            authorization_header: Some("Bearer override".to_string()),
            vertexai_service_account_json: None,
            extra_headers: HashMap::new(),
            additional_headers: HashMap::new(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        };

        let mut request = ProviderRequest::get(Url::parse("https://example.com").unwrap());
        apply_gemini_auth(&mut request, &config).unwrap();

        assert_eq!(
            request.header_value("authorization"),
            Some("Bearer override")
        );
        assert_eq!(request.header_value("x-goog-api-key"), None);
        assert_eq!(request.url().query(), None);
    }
}
