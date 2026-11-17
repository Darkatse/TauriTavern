use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, RequestBuilder, Url};
use serde_json::Value;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::append_endpoint_path;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionRepository, ChatCompletionRepositoryGenerateResponse,
    ChatCompletionSource, ChatCompletionStreamDelta,
};

use crate::provider_response;

mod aws_bedrock;
mod claude;
mod cohere;
mod gemini;
mod gemini_interactions;
mod makersuite;
#[cfg(test)]
mod makersuite_tests;
mod normalizers;
mod openai;
mod openai_responses;
mod pollinations;
mod vertexai;
pub(crate) mod vertexai_auth;
mod workers_ai;
mod xai;

#[derive(Debug, Clone, Copy)]
struct PromptCachePerformanceUsage {
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
    input_tokens: u64,
}

pub struct HttpChatCompletionRepository {
    http_clients: Arc<HttpClientPool>,
    openai_responses_ws_sessions: openai_responses::ResponsesWsSessionPool,
}

impl HttpChatCompletionRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self {
            http_clients,
            openai_responses_ws_sessions: openai_responses::ResponsesWsSessionPool::default(),
        }
    }

    fn client(&self, config: &ChatCompletionApiConfig) -> Result<Client, DomainError> {
        self.client_for_profile(config, HttpClientProfile::Generation)
    }

    fn metadata_client(&self, config: &ChatCompletionApiConfig) -> Result<Client, DomainError> {
        self.client_for_profile(config, HttpClientProfile::ProviderMetadata)
    }

    fn client_for_profile(
        &self,
        config: &ChatCompletionApiConfig,
        profile: HttpClientProfile,
    ) -> Result<Client, DomainError> {
        if config.user_configured_endpoint {
            self.http_clients
                .user_endpoint_client(profile, &config.base_url)
        } else {
            self.http_clients.client(profile)
        }
    }

    fn stream_client(&self, config: &ChatCompletionApiConfig) -> Result<Client, DomainError> {
        self.client_for_profile(config, HttpClientProfile::GenerationStream)
    }

    fn websocket_client(
        &self,
        config: &ChatCompletionApiConfig,
    ) -> Result<(Client, u64), DomainError> {
        if config.user_configured_endpoint {
            self.http_clients.user_endpoint_client_with_revision(
                HttpClientProfile::ChatCompletionWebSocket,
                &config.base_url,
            )
        } else {
            self.http_clients
                .client_with_revision(HttpClientProfile::ChatCompletionWebSocket)
        }
    }

    fn build_url(base_url: &str, path: &str) -> Result<Url, DomainError> {
        append_endpoint_path(base_url, path)
    }

    fn apply_bearer_auth(request: RequestBuilder, api_key: &str) -> RequestBuilder {
        if api_key.trim().is_empty() {
            request
        } else {
            request.header(AUTHORIZATION, format!("Bearer {api_key}"))
        }
    }

    fn apply_openai_auth(
        request: RequestBuilder,
        config: &ChatCompletionApiConfig,
    ) -> RequestBuilder {
        if let Some(authorization_header) = config.authorization_header.as_deref() {
            Self::apply_header_if_present(request, "Authorization", authorization_header)
        } else {
            Self::apply_bearer_auth(request, &config.api_key)
        }
    }

    fn apply_header_if_present(
        request: RequestBuilder,
        header_name: &str,
        header_value: &str,
    ) -> RequestBuilder {
        if header_value.trim().is_empty() {
            request
        } else {
            request.header(header_name, header_value)
        }
    }

    fn apply_extra_headers(
        request: RequestBuilder,
        headers: &HashMap<String, String>,
    ) -> RequestBuilder {
        Self::apply_extra_headers_with_filter(request, headers, |_, _| false)
    }

    fn apply_additional_headers(
        request: RequestBuilder,
        config: &ChatCompletionApiConfig,
    ) -> RequestBuilder {
        Self::apply_extra_headers(request, &config.additional_headers)
    }

    fn apply_extra_headers_with_filter<F>(
        request: RequestBuilder,
        headers: &HashMap<String, String>,
        mut should_skip: F,
    ) -> RequestBuilder
    where
        F: FnMut(&str, &str) -> bool,
    {
        let mut header_map = HeaderMap::new();

        for (key, value) in headers {
            if should_skip(key, value) {
                continue;
            }

            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() {
                continue;
            }

            let header_name = match HeaderName::from_bytes(key.as_bytes()) {
                Ok(header_name) => header_name,
                Err(_) => return request.header(key, value),
            };
            let header_value = match HeaderValue::from_str(value) {
                Ok(header_value) => header_value,
                Err(_) => return request.header(header_name, value),
            };

            header_map.insert(header_name, header_value);
        }

        if header_map.is_empty() {
            request
        } else {
            request.headers(header_map)
        }
    }

    async fn stream_sse_response_with_cache_logging(
        provider_name: &str,
        model: String,
        response: reqwest::Response,
        sender: GenerationStreamSender,
        cancel: GenerationCancelReceiver,
    ) -> Result<(), DomainError> {
        let mut logged = false;
        provider_response::consume_sse_until_cancelled(provider_name, response, cancel, |payload| {
            if !logged {
                logged = log_prompt_cache_performance_in_event(provider_name, &model, payload);
            }
            provider_response::send_event(&sender, payload)
        })
        .await
    }
}

fn log_prompt_cache_performance_in_event(provider_name: &str, model: &str, payload: &[u8]) -> bool {
    let has_cache_usage = [
        b"cache_read_input_tokens".as_slice(),
        b"cache_creation_input_tokens".as_slice(),
    ]
    .into_iter()
    .any(|field| payload.windows(field.len()).any(|window| window == field));
    if !has_cache_usage {
        return false;
    }

    let Ok(value) = serde_json::from_slice::<Value>(payload) else {
        return false;
    };
    log_prompt_cache_performance_if_present(provider_name, Some(model), &value)
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

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn log_prompt_cache_performance_if_present(
    provider_name: &str,
    model: Option<&str>,
    value: &Value,
) -> bool {
    let Some(usage) = find_prompt_cache_performance_usage(value) else {
        return false;
    };

    let total_input_tokens =
        usage.cache_creation_input_tokens + usage.cache_read_input_tokens + usage.input_tokens;

    match model.map(str::trim).filter(|value| !value.is_empty()) {
        Some(model) => {
            tracing::info!(
                "{provider_name} prompt cache usage: model={model} cache_read_input_tokens={} cache_creation_input_tokens={} input_tokens={} total_input_tokens={}",
                usage.cache_read_input_tokens,
                usage.cache_creation_input_tokens,
                usage.input_tokens,
                total_input_tokens,
            );
        }
        None => {
            tracing::info!(
                "{provider_name} prompt cache usage: cache_read_input_tokens={} cache_creation_input_tokens={} input_tokens={} total_input_tokens={}",
                usage.cache_read_input_tokens,
                usage.cache_creation_input_tokens,
                usage.input_tokens,
                total_input_tokens,
            );
        }
    }

    true
}

fn find_prompt_cache_performance_usage(value: &Value) -> Option<PromptCachePerformanceUsage> {
    if let Some(usage) = value.get("usage").and_then(Value::as_object)
        && let Some(parsed) = parse_prompt_cache_performance_usage(usage)
    {
        return Some(parsed);
    }

    if let Some(message_usage) = value
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("usage"))
        .and_then(Value::as_object)
        && let Some(parsed) = parse_prompt_cache_performance_usage(message_usage)
    {
        return Some(parsed);
    }

    None
}

fn parse_prompt_cache_performance_usage(
    usage: &serde_json::Map<String, Value>,
) -> Option<PromptCachePerformanceUsage> {
    let cache_creation_input_tokens = value_to_u64(usage.get("cache_creation_input_tokens"))?;
    let cache_read_input_tokens = value_to_u64(usage.get("cache_read_input_tokens"))?;
    let input_tokens = value_to_u64(usage.get("input_tokens"))?;

    Some(PromptCachePerformanceUsage {
        cache_creation_input_tokens,
        cache_read_input_tokens,
        input_tokens,
    })
}

fn value_to_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

fn provider_transport_source(
    source: ChatCompletionSource,
    endpoint_path: &str,
) -> Result<ChatCompletionSource, DomainError> {
    if source == ChatCompletionSource::Custom
        && matches!(endpoint_path, "/generateContent" | "/streamGenerateContent")
    {
        return Ok(ChatCompletionSource::Makersuite);
    }

    if source != ChatCompletionSource::OpenCode {
        return Ok(source);
    }

    match endpoint_path {
        "/chat/completions" | "/responses" => Ok(ChatCompletionSource::OpenAi),
        "/messages" => Ok(ChatCompletionSource::Claude),
        "/generateContent" | "/streamGenerateContent" => Ok(ChatCompletionSource::Makersuite),
        path => Err(DomainError::InvalidData(format!(
            "Unsupported OpenCode endpoint path: {path}"
        ))),
    }
}

#[async_trait]
impl ChatCompletionRepository for HttpChatCompletionRepository {
    async fn list_models(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
    ) -> Result<Value, DomainError> {
        let source_name = source.display_name();

        match source {
            ChatCompletionSource::OpenAi
            | ChatCompletionSource::OpenCode
            | ChatCompletionSource::OpenRouter
            | ChatCompletionSource::Custom
            | ChatCompletionSource::DeepSeek
            | ChatCompletionSource::Groq
            | ChatCompletionSource::Moonshot
            | ChatCompletionSource::Chutes
            | ChatCompletionSource::Zai => openai::list_models(self, config, source_name).await,
            ChatCompletionSource::SiliconFlow => {
                openai::list_models_with_path(
                    self,
                    config,
                    source_name,
                    "/models?type=text&sub_type=chat",
                )
                .await
            }
            ChatCompletionSource::WorkersAi => workers_ai::list_models(self, config).await,
            ChatCompletionSource::Pollinations => pollinations::list_models(self, config).await,
            ChatCompletionSource::Xai => xai::list_models(self, config).await,
            ChatCompletionSource::Cohere => cohere::list_models(self, config).await,
            ChatCompletionSource::NanoGpt => {
                openai::list_models_with_path(self, config, source_name, "/models?detailed=true")
                    .await
            }
            ChatCompletionSource::MiniMax => Err(DomainError::InvalidData(
                "MiniMax does not expose dynamic model listing; status bypass belongs to the application service".to_string(),
            )),
            ChatCompletionSource::AwsBedrock => aws_bedrock::list_models(self, config).await,
            ChatCompletionSource::Claude => claude::list_models(self, config).await,
            ChatCompletionSource::Makersuite => makersuite::list_models(self, config).await,
            ChatCompletionSource::VertexAi => vertexai::list_models(self, config).await,
        }
    }

    async fn generate(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
    ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
        let source_name = source.display_name();
        let source = provider_transport_source(source, endpoint_path)?;

        match (source, endpoint_path) {
            (ChatCompletionSource::OpenAi, "/responses") => {
                openai_responses::generate(self, config, endpoint_path, payload, "OpenAI Responses")
                    .await
            }
            (ChatCompletionSource::Custom, "/responses") => {
                openai_responses::generate(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom OpenAI Responses",
                )
                .await
            }
            (ChatCompletionSource::Custom, "/interactions") => {
                gemini_interactions::generate(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Gemini Interactions",
                )
                .await
            }
            (ChatCompletionSource::Custom, "/messages") => {
                claude::generate(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Claude Messages",
                )
                .await
            }
            (
                ChatCompletionSource::OpenAi
                | ChatCompletionSource::OpenRouter
                | ChatCompletionSource::Custom
                | ChatCompletionSource::DeepSeek
                | ChatCompletionSource::Groq
                | ChatCompletionSource::Moonshot
                | ChatCompletionSource::NanoGpt
                | ChatCompletionSource::Chutes
                | ChatCompletionSource::SiliconFlow
                | ChatCompletionSource::WorkersAi
                | ChatCompletionSource::Zai
                | ChatCompletionSource::MiniMax
                | ChatCompletionSource::Xai
                | ChatCompletionSource::Pollinations,
                _,
            ) => openai::generate(self, config, endpoint_path, payload, source_name)
                .await
                .map(ChatCompletionRepositoryGenerateResponse::from_body),
            (ChatCompletionSource::Cohere, _) => {
                cohere::generate(self, config, endpoint_path, payload)
                    .await
                    .map(ChatCompletionRepositoryGenerateResponse::from_body)
            }
            (ChatCompletionSource::Claude, _) => {
                claude::generate(self, config, endpoint_path, payload, source_name).await
            }
            (ChatCompletionSource::AwsBedrock, _) => {
                aws_bedrock::generate(self, config, endpoint_path, payload).await
            }
            (ChatCompletionSource::Makersuite, _) => {
                makersuite::generate(self, config, endpoint_path, payload).await
            }
            (ChatCompletionSource::VertexAi, _) => {
                vertexai::generate(self, config, endpoint_path, payload).await
            }
            (ChatCompletionSource::OpenCode, _) => {
                unreachable!("OpenCode source is resolved to its wire transport before dispatch")
            }
        }
    }

    async fn generate_stream(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        sender: GenerationStreamSender,
        cancel: GenerationCancelReceiver,
    ) -> Result<(), DomainError> {
        let source_name = source.display_name();
        let source = provider_transport_source(source, endpoint_path)?;

        match (source, endpoint_path) {
            (ChatCompletionSource::OpenAi, "/responses") => {
                openai_responses::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "OpenAI Responses",
                    sender,
                    cancel,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/responses") => {
                openai_responses::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom OpenAI Responses",
                    sender,
                    cancel,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/interactions") => {
                gemini_interactions::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Gemini Interactions",
                    sender,
                    cancel,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/messages") => {
                claude::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Claude Messages",
                    sender,
                    cancel,
                )
                .await
            }
            (
                ChatCompletionSource::OpenAi
                | ChatCompletionSource::OpenRouter
                | ChatCompletionSource::Custom
                | ChatCompletionSource::DeepSeek
                | ChatCompletionSource::Groq
                | ChatCompletionSource::Moonshot
                | ChatCompletionSource::NanoGpt
                | ChatCompletionSource::Chutes
                | ChatCompletionSource::SiliconFlow
                | ChatCompletionSource::WorkersAi
                | ChatCompletionSource::Zai
                | ChatCompletionSource::MiniMax
                | ChatCompletionSource::Xai
                | ChatCompletionSource::Pollinations,
                _,
            ) => {
                openai::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    source_name,
                    sender,
                    cancel,
                )
                .await
            }
            (ChatCompletionSource::Cohere, _) => {
                cohere::generate_stream(self, config, endpoint_path, payload, sender, cancel).await
            }
            (ChatCompletionSource::Claude, _) => {
                claude::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    source_name,
                    sender,
                    cancel,
                )
                .await
            }
            (ChatCompletionSource::AwsBedrock, _) => {
                aws_bedrock::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
            }
            (ChatCompletionSource::Makersuite, _) => {
                makersuite::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
            }
            (ChatCompletionSource::VertexAi, _) => {
                vertexai::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
            }
            (ChatCompletionSource::OpenCode, _) => {
                unreachable!("OpenCode source is resolved to its wire transport before dispatch")
            }
        }
    }

    async fn generate_with_deltas(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        on_delta: &mut (dyn FnMut(ChatCompletionStreamDelta) + Send),
    ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
        let source_name = source.display_name();
        let source = provider_transport_source(source, endpoint_path)?;

        match (source, endpoint_path) {
            (ChatCompletionSource::OpenAi, "/responses") => {
                openai_responses::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "OpenAI Responses",
                    on_delta,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/responses") => {
                openai_responses::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom OpenAI Responses",
                    on_delta,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/interactions") => {
                gemini_interactions::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Gemini Interactions",
                    on_delta,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/messages") => {
                claude::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Claude Messages",
                    on_delta,
                )
                .await
            }
            (ChatCompletionSource::Claude, _) => {
                claude::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    source_name,
                    on_delta,
                )
                .await
            }
            (ChatCompletionSource::Makersuite, _) => {
                makersuite::generate_with_deltas(self, config, endpoint_path, payload, on_delta)
                    .await
            }
            (ChatCompletionSource::VertexAi, _) => {
                vertexai::generate_with_deltas(self, config, endpoint_path, payload, on_delta).await
            }
            (ChatCompletionSource::AwsBedrock, _) => {
                aws_bedrock::generate_with_deltas(self, config, endpoint_path, payload, on_delta)
                    .await
            }
            (
                ChatCompletionSource::OpenAi
                | ChatCompletionSource::OpenRouter
                | ChatCompletionSource::Custom
                | ChatCompletionSource::DeepSeek
                | ChatCompletionSource::Groq
                | ChatCompletionSource::Moonshot
                | ChatCompletionSource::NanoGpt
                | ChatCompletionSource::Chutes
                | ChatCompletionSource::SiliconFlow
                | ChatCompletionSource::WorkersAi
                | ChatCompletionSource::Zai
                | ChatCompletionSource::MiniMax
                | ChatCompletionSource::Xai
                | ChatCompletionSource::Pollinations,
                "/chat/completions",
            ) => {
                openai::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    source_name,
                    on_delta,
                )
                .await
            }
            (source, endpoint_path) => Err(DomainError::InvalidData(format!(
                "Tool-call delta streaming is unsupported for source `{}` endpoint `{endpoint_path}`",
                source.key()
            ))),
        }
    }

    async fn close_provider_session(&self, session_id: &str) {
        self.openai_responses_ws_sessions.close(session_id).await;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reqwest::Client;
    use reqwest::header::AUTHORIZATION;

    use tt_ports::repositories::chat_completion_repository::{
        ChatCompletionApiConfig, ChatCompletionSource,
    };

    use super::{HttpChatCompletionRepository, provider_transport_source};

    #[test]
    fn opencode_routes_by_wire_format() {
        assert_eq!(
            provider_transport_source(ChatCompletionSource::OpenCode, "/messages").unwrap(),
            ChatCompletionSource::Claude
        );
        assert_eq!(
            provider_transport_source(ChatCompletionSource::OpenCode, "/streamGenerateContent")
                .unwrap(),
            ChatCompletionSource::Makersuite
        );
        assert!(provider_transport_source(ChatCompletionSource::OpenCode, "/unknown").is_err());
    }

    #[test]
    fn apply_extra_headers_with_filter_skips_matching_headers() {
        let mut headers = HashMap::new();
        headers.insert("anthropic-beta".to_string(), "prompt-caching".to_string());
        headers.insert("x-custom".to_string(), "value".to_string());

        let request = Client::new().get("https://example.com");
        let request = HttpChatCompletionRepository::apply_extra_headers_with_filter(
            request,
            &headers,
            |key, _| key.eq_ignore_ascii_case("anthropic-beta"),
        );
        let request = request.build().expect("request should build");

        assert!(request.headers().get("anthropic-beta").is_none());
        assert_eq!(
            request
                .headers()
                .get("x-custom")
                .and_then(|value| value.to_str().ok()),
            Some("value")
        );
    }

    #[test]
    fn apply_openai_auth_prefers_explicit_authorization_header() {
        let config = ChatCompletionApiConfig {
            base_url: "https://example.com/v1".to_string(),
            user_configured_endpoint: false,
            api_key: "saved-secret".to_string(),
            authorization_header: Some("Bearer override".to_string()),
            vertexai_service_account_json: None,
            extra_headers: HashMap::new(),
            additional_headers: HashMap::new(),
            anthropic_beta_header_mode:
                tt_ports::repositories::chat_completion_repository::AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        };

        let request = Client::new().get("https://example.com");
        let request = HttpChatCompletionRepository::apply_openai_auth(request, &config);
        let request = request.build().expect("request should build");

        let values = request
            .headers()
            .get_all(AUTHORIZATION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>();

        assert_eq!(values, vec!["Bearer override"]);
    }

    #[test]
    fn additional_headers_replace_existing_header_values() {
        let config = ChatCompletionApiConfig {
            base_url: "https://example.com/v1".to_string(),
            user_configured_endpoint: false,
            api_key: "saved-secret".to_string(),
            authorization_header: None,
            vertexai_service_account_json: None,
            extra_headers: HashMap::new(),
            additional_headers: HashMap::from([(
                "Authorization".to_string(),
                "Bearer final".to_string(),
            )]),
            anthropic_beta_header_mode:
                tt_ports::repositories::chat_completion_repository::AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        };

        let request = Client::new().get("https://example.com");
        let request = HttpChatCompletionRepository::apply_openai_auth(request, &config);
        let request = HttpChatCompletionRepository::apply_additional_headers(request, &config);
        let request = request.build().expect("request should build");

        let values = request
            .headers()
            .get_all(AUTHORIZATION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>();

        assert_eq!(values, vec!["Bearer final"]);
    }
}
