use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use serde_json::Value;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_domain::models::endpoint_url::append_endpoint_path;
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionRepository,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionSource, ChatCompletionStreamSender,
    ChatCompletionToolCallDelta,
};

mod aws_bedrock;
mod claude;
mod cohere;
mod gemini;
mod gemini_interactions;
mod makersuite;
mod normalizers;
mod openai;
mod openai_responses;
mod response_body;
mod vertexai;
pub(crate) mod vertexai_auth;
mod workers_ai;

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

#[derive(Default)]
struct SseEventAccumulator {
    data: Vec<u8>,
}

impl SseEventAccumulator {
    fn on_line<F: FnMut(&[u8]) -> Result<(), DomainError>>(
        &mut self,
        line: &[u8],
        on_event: &mut F,
    ) -> Result<(), DomainError> {
        if line.is_empty() {
            return self.dispatch(on_event);
        }

        if line.first().is_some_and(|byte| *byte == b':') {
            return Ok(());
        }

        let (field, value) = split_sse_field(line);
        if field == b"data" {
            if !self.data.is_empty() {
                self.data.push(b'\n');
            }
            self.data.extend_from_slice(value);
        }

        Ok(())
    }

    fn finish<F: FnMut(&[u8]) -> Result<(), DomainError>>(
        &mut self,
        on_event: &mut F,
    ) -> Result<(), DomainError> {
        self.dispatch(on_event)
    }

    fn dispatch<F: FnMut(&[u8]) -> Result<(), DomainError>>(
        &mut self,
        on_event: &mut F,
    ) -> Result<(), DomainError> {
        if self.data.is_empty() {
            return Ok(());
        }

        let payload = std::mem::take(&mut self.data);
        on_event(payload.as_slice())
    }
}

fn split_sse_field(line: &[u8]) -> (&[u8], &[u8]) {
    let Some(colon_index) = line.iter().position(|byte| *byte == b':') else {
        return (line, b"");
    };

    let field = &line[..colon_index];
    let mut value = &line[colon_index + 1..];
    let value_start = value
        .iter()
        .position(|byte| *byte != b' ' && *byte != b'\t')
        .unwrap_or(value.len());
    value = &value[value_start..];

    (field, value)
}

impl HttpChatCompletionRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self {
            http_clients,
            openai_responses_ws_sessions: openai_responses::ResponsesWsSessionPool::default(),
        }
    }

    fn client(&self, config: &ChatCompletionApiConfig) -> Result<Client, DomainError> {
        self.client_for_profile(config, HttpClientProfile::ChatCompletion)
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
        self.client_for_profile(config, HttpClientProfile::ChatCompletionStream)
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

    async fn send_checked(
        request: RequestBuilder,
        provider_name: &str,
        error_context: &str,
    ) -> Result<reqwest::Response, DomainError> {
        let response = request
            .send()
            .await
            .map_err(|error| Self::map_transport_error(error_context, error))?;

        if !response.status().is_success() {
            return Err(Self::map_error_response(provider_name, response, error_context).await);
        }

        Ok(response)
    }

    async fn map_error_response(
        provider_name: &str,
        response: reqwest::Response,
        default_message: &str,
    ) -> DomainError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Self::map_error_status(provider_name, status, &body, default_message)
    }

    fn map_error_status(
        provider_name: &str,
        status: StatusCode,
        body: &str,
        default_message: &str,
    ) -> DomainError {
        let message = extract_error_message(body, default_message);

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                DomainError::AuthenticationError(message)
            }
            StatusCode::BAD_REQUEST => DomainError::InvalidData(message),
            StatusCode::TOO_MANY_REQUESTS => DomainError::rate_limited(format!(
                "{provider_name} endpoint failed with status {}: {message}",
                status.as_u16()
            )),
            status if is_retryable_status(status) => DomainError::transient(format!(
                "{provider_name} endpoint failed with status {}: {message}",
                status.as_u16()
            )),
            _ => DomainError::InternalError(format!(
                "{provider_name} endpoint failed with status {}: {message}",
                status.as_u16()
            )),
        }
    }

    fn map_transport_error(label: &str, error: reqwest::Error) -> DomainError {
        let failure = crate::http_error::reqwest_transport_failure(&error);
        tracing::warn!(
            operation = label,
            code = %failure.code,
            category = %failure.category,
            endpoint = failure.endpoint.as_deref().unwrap_or(""),
            timeout = error.is_timeout(),
            connect = error.is_connect(),
            body = error.is_body(),
            request = error.is_request(),
            "upstream transport request failed",
        );
        DomainError::upstream_failure(failure)
    }

    async fn stream_sse_response(
        provider_name: &str,
        response: reqwest::Response,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), DomainError> {
        Self::stream_sse_response_with_hook(provider_name, response, sender, cancel, |_| Ok(()))
            .await
    }

    async fn stream_sse_response_with_hook<F>(
        provider_name: &str,
        response: reqwest::Response,
        sender: ChatCompletionStreamSender,
        mut cancel: ChatCompletionCancelReceiver,
        mut hook: F,
    ) -> Result<(), DomainError>
    where
        F: FnMut(&[u8]) -> Result<(), DomainError>,
    {
        let consume = Self::consume_sse_response(provider_name, response, |payload| {
            hook(payload)?;
            let payload = std::str::from_utf8(payload).map_err(|error| {
                DomainError::InternalError(format!("SSE payload is not valid UTF-8: {error}"))
            })?;
            let _ = sender.send(payload.to_string());
            Ok(())
        });
        tokio::pin!(consume);

        tokio::select! {
            result = &mut consume => result,
            Ok(_) = cancel.wait_for(|cancelled| *cancelled) => Ok(()),
        }
    }

    async fn stream_sse_response_with_cache_logging(
        provider_name: &str,
        model: String,
        response: reqwest::Response,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), DomainError> {
        let mut logged = false;
        Self::stream_sse_response_with_hook(
            provider_name,
            response,
            sender,
            cancel,
            move |payload| {
                if logged {
                    return Ok(());
                }

                let has_cache_usage = [
                    b"cache_read_input_tokens".as_slice(),
                    b"cache_creation_input_tokens".as_slice(),
                ]
                .into_iter()
                .any(|field| payload.windows(field.len()).any(|window| window == field));
                if !has_cache_usage {
                    return Ok(());
                }

                let Ok(value) = serde_json::from_slice::<Value>(payload) else {
                    return Ok(());
                };
                logged = log_prompt_cache_performance_if_present(
                    provider_name,
                    Some(model.as_str()),
                    &value,
                );
                Ok(())
            },
        )
        .await
    }

    async fn consume_sse_response<F>(
        provider_name: &str,
        mut response: reqwest::Response,
        mut on_event: F,
    ) -> Result<(), DomainError>
    where
        F: FnMut(&[u8]) -> Result<(), DomainError>,
    {
        let mut buffer = Vec::<u8>::new();
        let mut accumulator = SseEventAccumulator::default();
        let endpoint = response.url().clone();

        loop {
            let chunk = response.chunk().await.map_err(|error| {
                let failure = crate::http_error::reqwest_body_failure(&error, Some(&endpoint));
                tracing::warn!(
                    provider = provider_name,
                    operation = "stream",
                    code = %failure.code,
                    category = %failure.category,
                    endpoint = failure.endpoint.as_deref().unwrap_or(""),
                    timeout = error.is_timeout(),
                    connect = error.is_connect(),
                    body = error.is_body(),
                    request = error.is_request(),
                    "upstream stream read failed",
                );
                DomainError::upstream_failure(failure)
            })?;

            let Some(chunk) = chunk else {
                break;
            };

            buffer.extend_from_slice(&chunk);
            Self::forward_sse_events(&mut buffer, &mut accumulator, &mut on_event)?;
        }

        if !buffer.is_empty() {
            Self::forward_sse_events(&mut buffer, &mut accumulator, &mut on_event)?;
            Self::forward_sse_line(buffer.as_slice(), &mut accumulator, &mut on_event)?;
            buffer.clear();
        }

        accumulator.finish(&mut on_event)?;
        Ok(())
    }

    fn forward_sse_events<F: FnMut(&[u8]) -> Result<(), DomainError>>(
        buffer: &mut Vec<u8>,
        accumulator: &mut SseEventAccumulator,
        on_event: &mut F,
    ) -> Result<(), DomainError> {
        let mut line_start = 0_usize;
        let mut consumed = 0_usize;

        for (index, byte) in buffer.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }

            let mut line = &buffer[line_start..index];
            if line.last().is_some_and(|byte| *byte == b'\r') {
                line = &line[..line.len() - 1];
            }

            accumulator.on_line(line, on_event)?;
            consumed = index + 1;
            line_start = consumed;
        }

        if consumed > 0 {
            buffer.drain(..consumed);
        }

        Ok(())
    }

    fn forward_sse_line<F: FnMut(&[u8]) -> Result<(), DomainError>>(
        line: &[u8],
        accumulator: &mut SseEventAccumulator,
        on_event: &mut F,
    ) -> Result<(), DomainError> {
        let mut line = line;
        if line.last().is_some_and(|byte| *byte == b'\r') {
            line = &line[..line.len() - 1];
        }

        accumulator.on_line(line, on_event)
    }
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

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
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
                | ChatCompletionSource::MiniMax,
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
        }
    }

    async fn generate_stream(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), DomainError> {
        let source_name = source.display_name();

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
                | ChatCompletionSource::MiniMax,
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
        }
    }

    async fn generate_with_tool_call_deltas(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        on_tool_call_delta: &mut (dyn FnMut(ChatCompletionToolCallDelta) + Send),
    ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
        let source_name = source.display_name();

        match (source, endpoint_path) {
            (ChatCompletionSource::OpenAi, "/responses") => {
                openai_responses::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "OpenAI Responses",
                    on_tool_call_delta,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/responses") => {
                openai_responses::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom OpenAI Responses",
                    on_tool_call_delta,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/interactions") => {
                gemini_interactions::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Gemini Interactions",
                    on_tool_call_delta,
                )
                .await
            }
            (ChatCompletionSource::Custom, "/messages") => {
                claude::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    "Custom Claude Messages",
                    on_tool_call_delta,
                )
                .await
            }
            (ChatCompletionSource::Claude, _) => {
                claude::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    source_name,
                    on_tool_call_delta,
                )
                .await
            }
            (ChatCompletionSource::Makersuite, _) => {
                makersuite::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    on_tool_call_delta,
                )
                .await
            }
            (ChatCompletionSource::VertexAi, _) => {
                vertexai::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    on_tool_call_delta,
                )
                .await
            }
            (ChatCompletionSource::AwsBedrock, _) => {
                aws_bedrock::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    on_tool_call_delta,
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
                | ChatCompletionSource::MiniMax,
                "/chat/completions",
            ) => {
                openai::generate_with_tool_call_deltas(
                    self,
                    config,
                    endpoint_path,
                    payload,
                    source_name,
                    on_tool_call_delta,
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

fn extract_error_message(body: &str, default_message: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return default_message.to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(body)
        && let Some(message) = value
            .pointer("/error/message")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        return message.to_string();
    }

    body.to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reqwest::Client;
    use reqwest::header::AUTHORIZATION;

    use tt_domain::errors::DomainError;
    use tt_ports::repositories::chat_completion_repository::ChatCompletionApiConfig;

    use super::HttpChatCompletionRepository;

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

    #[test]
    fn error_status_classification_marks_retryable_provider_failures() {
        let rate_limited = HttpChatCompletionRepository::map_error_status(
            "OpenAI",
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"slow down"}}"#,
            "Generation request failed",
        );
        assert!(matches!(rate_limited, DomainError::RateLimited { .. }));

        let gateway_timeout = HttpChatCompletionRepository::map_error_status(
            "OpenAI",
            reqwest::StatusCode::BAD_GATEWAY,
            "upstream unavailable",
            "Generation request failed",
        );
        assert!(matches!(gateway_timeout, DomainError::Transient(_)));

        let bad_request = HttpChatCompletionRepository::map_error_status(
            "OpenAI",
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"bad payload"}}"#,
            "Generation request failed",
        );
        assert!(matches!(bad_request, DomainError::InvalidData(_)));
    }

    #[test]
    fn forward_sse_events_extracts_data_payloads() {
        let mut events = Vec::<Vec<u8>>::new();
        let mut buffer =
            b"event: message\r\ndata: {\"chunk\":1}\n\n: ping\ndata: [DONE]\n\n".to_vec();

        let mut on_event = |event: &[u8]| {
            events.push(event.to_vec());
            Ok(())
        };
        let mut accumulator = super::SseEventAccumulator::default();
        let result = HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &mut on_event,
        );
        assert!(result.is_ok());

        assert_eq!(events, [b"{\"chunk\":1}".to_vec(), b"[DONE]".to_vec()]);
        assert!(buffer.is_empty());
    }

    #[test]
    fn forward_sse_events_keeps_partial_line_in_buffer() {
        let mut events = Vec::<Vec<u8>>::new();
        let mut buffer = b"data: {\"chunk\":1}".to_vec();

        let mut on_event = |event: &[u8]| {
            events.push(event.to_vec());
            Ok(())
        };
        let mut accumulator = super::SseEventAccumulator::default();
        let result = HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &mut on_event,
        );
        assert!(result.is_ok());
        assert!(events.is_empty());
        assert_eq!(buffer, b"data: {\"chunk\":1}".to_vec());
    }

    #[test]
    fn forward_sse_events_combines_multiline_data_fields() {
        let mut events = Vec::<Vec<u8>>::new();
        let mut buffer = b"data: first\ndata: second\n\n".to_vec();

        let mut on_event = |event: &[u8]| {
            events.push(event.to_vec());
            Ok(())
        };
        let mut accumulator = super::SseEventAccumulator::default();
        HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &mut on_event,
        )
        .unwrap();

        assert_eq!(events, [b"first\nsecond".to_vec()]);
        assert!(buffer.is_empty());
    }

    #[test]
    fn forward_sse_events_can_flush_pending_event_at_end_of_stream() {
        let mut events = Vec::<Vec<u8>>::new();
        let mut buffer = b"data: tail\n".to_vec();

        let mut on_event = |event: &[u8]| {
            events.push(event.to_vec());
            Ok(())
        };
        let mut accumulator = super::SseEventAccumulator::default();
        HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &mut on_event,
        )
        .unwrap();

        accumulator.finish(&mut on_event).unwrap();

        assert_eq!(events, [b"tail".to_vec()]);
    }
}
