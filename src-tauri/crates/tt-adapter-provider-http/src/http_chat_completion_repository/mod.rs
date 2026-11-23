use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::{Client, Url};
use serde_json::Value;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_contracts::observability::RequestTransportDto;
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::append_endpoint_path;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionRepository, ChatCompletionRepositoryGenerateResponse,
    ChatCompletionSource, ChatCompletionStreamDelta, GenerationCall, WireRequest,
};

use crate::provider_request::{EVENT_STREAM, JSON, ProviderRequest};
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
#[cfg(test)]
mod request_description_tests;
#[cfg(test)]
mod test_upstream;
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

struct GenerationRequest<'a> {
    transport: RequestTransportDto,
    head: ProviderRequest,
    body: Cow<'a, Value>,
}

impl<'a> GenerationRequest<'a> {
    fn http(head: ProviderRequest, body: Cow<'a, Value>) -> Self {
        Self {
            transport: RequestTransportDto::Http,
            head,
            body,
        }
    }
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

    async fn send_generation(
        &self,
        config: &ChatCompletionApiConfig,
        request: GenerationRequest<'_>,
        stream: bool,
        provider_name: &str,
    ) -> Result<reqwest::Response, DomainError> {
        let profile = if stream {
            HttpClientProfile::GenerationStream
        } else {
            HttpClientProfile::Generation
        };
        let client = self.client_for_profile(config, profile)?;
        provider_response::send_checked(
            request.head.with_client(&client).json(&request.body),
            provider_name,
            "Generation request failed",
        )
        .await
    }

    fn apply_bearer_auth(request: &mut ProviderRequest, api_key: &str) -> Result<(), DomainError> {
        if api_key.trim().is_empty() {
            Ok(())
        } else {
            request.header("Authorization", &format!("Bearer {api_key}"))
        }
    }

    fn apply_authorization(
        request: &mut ProviderRequest,
        config: &ChatCompletionApiConfig,
    ) -> Result<(), DomainError> {
        if let Some(authorization_header) = config.authorization_header.as_deref() {
            Self::apply_header_if_present(request, "Authorization", authorization_header)
        } else {
            Self::apply_bearer_auth(request, &config.api_key)
        }
    }

    fn apply_header_if_present(
        request: &mut ProviderRequest,
        header_name: &str,
        header_value: &str,
    ) -> Result<(), DomainError> {
        if header_value.trim().is_empty() {
            Ok(())
        } else {
            request.header(header_name, header_value)
        }
    }

    fn apply_extra_headers(
        request: &mut ProviderRequest,
        headers: &HashMap<String, String>,
    ) -> Result<(), DomainError> {
        for (name, value) in headers {
            let (name, value) = (name.trim(), value.trim());
            if !name.is_empty() && !value.is_empty() {
                request.header(name, value)?;
            }
        }
        Ok(())
    }

    fn apply_additional_headers(
        request: &mut ProviderRequest,
        config: &ChatCompletionApiConfig,
    ) -> Result<(), DomainError> {
        Self::apply_extra_headers(request, &config.additional_headers)
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

#[derive(Clone, Copy)]
enum ProviderApi {
    OpenAi,
    OpenAiResponses,
    Claude,
    Makersuite,
    GeminiInteractions,
    VertexAi,
    Cohere,
    AwsBedrock,
}

/// The returned name identifies the wire API in errors and logs, not necessarily the source.
fn provider_api(
    source: ChatCompletionSource,
    endpoint_path: &str,
) -> Result<(ProviderApi, &'static str), DomainError> {
    let source_name = source.display_name();

    Ok(
        match (
            provider_transport_source(source, endpoint_path)?,
            endpoint_path,
        ) {
            (ChatCompletionSource::OpenAi, "/responses") => {
                (ProviderApi::OpenAiResponses, "OpenAI Responses")
            }
            (ChatCompletionSource::Custom, "/responses") => {
                (ProviderApi::OpenAiResponses, "Custom OpenAI Responses")
            }
            (ChatCompletionSource::Custom, "/interactions") => (
                ProviderApi::GeminiInteractions,
                "Custom Gemini Interactions",
            ),
            (ChatCompletionSource::Custom, "/messages") => {
                (ProviderApi::Claude, "Custom Claude Messages")
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
            ) => (ProviderApi::OpenAi, source_name),
            (ChatCompletionSource::Cohere, _) => (ProviderApi::Cohere, source_name),
            (ChatCompletionSource::Claude, _) => (ProviderApi::Claude, source_name),
            (ChatCompletionSource::AwsBedrock, _) => (ProviderApi::AwsBedrock, source_name),
            (ChatCompletionSource::Makersuite, _) => (ProviderApi::Makersuite, source_name),
            (ChatCompletionSource::VertexAi, _) => (ProviderApi::VertexAi, source_name),
            (ChatCompletionSource::OpenCode, _) => {
                unreachable!("OpenCode source is resolved to its wire transport before dispatch")
            }
        },
    )
}

fn response_format(stream: bool) -> &'static str {
    if stream { EVENT_STREAM } else { JSON }
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
        let (api, provider_name) = provider_api(source, endpoint_path)?;

        match api {
            ProviderApi::OpenAiResponses => {
                openai_responses::generate(self, config, endpoint_path, payload, provider_name)
                    .await
            }
            ProviderApi::GeminiInteractions => {
                gemini_interactions::generate(self, config, endpoint_path, payload, provider_name)
                    .await
            }
            ProviderApi::OpenAi => {
                openai::generate(self, config, endpoint_path, payload, provider_name)
                    .await
                    .map(ChatCompletionRepositoryGenerateResponse::from_body)
            }
            ProviderApi::Cohere => cohere::generate(self, config, endpoint_path, payload)
                .await
                .map(ChatCompletionRepositoryGenerateResponse::from_body),
            ProviderApi::Claude => {
                claude::generate(self, config, endpoint_path, payload, provider_name).await
            }
            ProviderApi::AwsBedrock => {
                aws_bedrock::generate(self, config, endpoint_path, payload).await
            }
            ProviderApi::Makersuite => {
                makersuite::generate(self, config, endpoint_path, payload).await
            }
            ProviderApi::VertexAi => vertexai::generate(self, config, endpoint_path, payload).await,
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
        let (api, provider_name) = provider_api(source, endpoint_path)?;

        match api {
            ProviderApi::OpenAiResponses => {
                openai_responses::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    sender,
                    cancel,
                )
                .await
            }
            ProviderApi::GeminiInteractions => {
                gemini_interactions::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    sender,
                    cancel,
                )
                .await
            }
            ProviderApi::OpenAi => {
                openai::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    sender,
                    cancel,
                )
                .await
            }
            ProviderApi::Cohere => {
                cohere::generate_stream(self, config, endpoint_path, payload, sender, cancel).await
            }
            ProviderApi::Claude => {
                claude::generate_stream(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    sender,
                    cancel,
                )
                .await
            }
            ProviderApi::AwsBedrock => {
                aws_bedrock::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
            }
            ProviderApi::Makersuite => {
                makersuite::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
            }
            ProviderApi::VertexAi => {
                vertexai::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
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
        let (api, provider_name) = provider_api(source, endpoint_path)?;

        match api {
            ProviderApi::OpenAiResponses => {
                openai_responses::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    on_delta,
                )
                .await
            }
            ProviderApi::GeminiInteractions => {
                gemini_interactions::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    on_delta,
                )
                .await
            }
            ProviderApi::OpenAi if endpoint_path == "/chat/completions" => {
                openai::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    on_delta,
                )
                .await
            }
            ProviderApi::Claude => {
                claude::generate_with_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    provider_name,
                    on_delta,
                )
                .await
            }
            ProviderApi::AwsBedrock => {
                aws_bedrock::generate_with_deltas(self, config, endpoint_path, payload, on_delta)
                    .await
            }
            ProviderApi::Makersuite => {
                makersuite::generate_with_deltas(self, config, endpoint_path, payload, on_delta)
                    .await
            }
            ProviderApi::VertexAi => {
                vertexai::generate_with_deltas(self, config, endpoint_path, payload, on_delta).await
            }
            ProviderApi::OpenAi | ProviderApi::Cohere => Err(DomainError::InvalidData(format!(
                "Tool-call delta streaming is unsupported for source `{}` endpoint `{endpoint_path}`",
                source.key()
            ))),
        }
    }

    async fn close_provider_session(&self, session_id: &str) {
        self.openai_responses_ws_sessions.close(session_id).await;
    }

    fn describe_request<'a>(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &'a Value,
        call: GenerationCall,
    ) -> Result<WireRequest<'a>, DomainError> {
        let (api, _) = provider_api(source, endpoint_path)?;
        let stream = call.is_stream();

        let request = match api {
            ProviderApi::OpenAiResponses => {
                openai_responses::generation_request(config, endpoint_path, payload, call)
            }
            ProviderApi::GeminiInteractions => {
                gemini_interactions::generation_request(config, endpoint_path, payload, stream)
            }
            ProviderApi::OpenAi => {
                openai::generation_request(config, endpoint_path, payload, stream)
            }
            ProviderApi::Cohere => {
                cohere::generation_request(config, endpoint_path, payload, stream)
            }
            ProviderApi::Claude => {
                claude::generation_request(config, endpoint_path, payload, stream)
            }
            ProviderApi::AwsBedrock => {
                aws_bedrock::generation_request(config, endpoint_path, payload, stream)
            }
            ProviderApi::Makersuite => {
                makersuite::generation_request(config, endpoint_path, payload, stream)
            }
            ProviderApi::VertexAi => {
                vertexai::unsigned_generation_request(config, endpoint_path, payload, stream)
            }
        }?;

        Ok(WireRequest {
            http: request.head.describe(
                request.transport,
                self.http_clients.product_user_agent(),
                &[
                    &config.api_key,
                    config.authorization_header.as_deref().unwrap_or_default(),
                ],
            ),
            body: request.body,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reqwest::Url;

    use tt_ports::repositories::chat_completion_repository::{
        AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionSource,
    };

    use super::{HttpChatCompletionRepository, provider_transport_source};
    use crate::provider_request::ProviderRequest;

    fn config(
        authorization_header: Option<&str>,
        additional_headers: &[(&str, &str)],
    ) -> ChatCompletionApiConfig {
        ChatCompletionApiConfig {
            base_url: "https://example.com/v1".to_string(),
            user_configured_endpoint: false,
            api_key: "saved-secret".to_string(),
            authorization_header: authorization_header.map(str::to_string),
            vertexai_service_account_json: None,
            extra_headers: HashMap::new(),
            additional_headers: additional_headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        }
    }

    fn request() -> ProviderRequest {
        ProviderRequest::get(Url::parse("https://example.com").unwrap())
    }

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
    fn apply_authorization_prefers_explicit_authorization_header() {
        let config = config(Some("Bearer override"), &[]);

        let mut request = request();
        HttpChatCompletionRepository::apply_authorization(&mut request, &config).unwrap();

        assert_eq!(
            request.header_value("authorization"),
            Some("Bearer override")
        );
    }

    #[test]
    fn additional_headers_replace_existing_header_values() {
        let config = config(None, &[("Authorization", "Bearer final")]);

        let mut request = request();
        HttpChatCompletionRepository::apply_authorization(&mut request, &config).unwrap();
        HttpChatCompletionRepository::apply_additional_headers(&mut request, &config).unwrap();

        assert_eq!(request.header_value("authorization"), Some("Bearer final"));
    }
}
