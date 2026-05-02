use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::header::AUTHORIZATION;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionRepository,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionSource, ChatCompletionStreamSender,
};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};

mod claude;
mod cohere;
mod gemini_interactions;
mod makersuite;
mod normalizers;
mod openai;
mod openai_responses;
mod vertexai;

#[derive(Debug, Clone, Copy)]
struct PromptCachePerformanceUsage {
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
    input_tokens: u64,
}

pub struct HttpChatCompletionRepository {
    http_clients: Arc<HttpClientPool>,
    openai_responses_previous_response_id_by_call_id: Mutex<HashMap<String, String>>,
}

#[derive(Default)]
struct SseEventAccumulator {
    data: Vec<u8>,
}

impl SseEventAccumulator {
    fn on_line<F: FnMut(&[u8])>(
        &mut self,
        line: &[u8],
        sender: &ChatCompletionStreamSender,
        hook: &mut F,
    ) -> Result<(), DomainError> {
        if line.is_empty() {
            return self.dispatch(sender, hook);
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

    fn finish<F: FnMut(&[u8])>(
        &mut self,
        sender: &ChatCompletionStreamSender,
        hook: &mut F,
    ) -> Result<(), DomainError> {
        self.dispatch(sender, hook)
    }

    fn dispatch<F: FnMut(&[u8])>(
        &mut self,
        sender: &ChatCompletionStreamSender,
        hook: &mut F,
    ) -> Result<(), DomainError> {
        if self.data.is_empty() {
            return Ok(());
        }

        let payload = std::mem::take(&mut self.data);
        hook(payload.as_slice());

        let payload = std::str::from_utf8(payload.as_slice()).map_err(|error| {
            DomainError::InternalError(format!("SSE payload is not valid UTF-8: {error}"))
        })?;

        if sender.send(payload.to_string()).is_err() {
            return Ok(());
        }

        Ok(())
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
            openai_responses_previous_response_id_by_call_id: Mutex::new(HashMap::new()),
        }
    }

    fn client(&self) -> Result<Client, DomainError> {
        self.http_clients.client(HttpClientProfile::ChatCompletion)
    }

    fn stream_client(&self) -> Result<Client, DomainError> {
        self.http_clients
            .client(HttpClientProfile::ChatCompletionStream)
    }

    fn build_url(base_url: &str, path: &str) -> String {
        format!("{}{}", base_url.trim_end_matches('/'), path)
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

    fn apply_extra_headers_with_filter<F>(
        mut request: RequestBuilder,
        headers: &HashMap<String, String>,
        mut should_skip: F,
    ) -> RequestBuilder
    where
        F: FnMut(&str, &str) -> bool,
    {
        for (key, value) in headers {
            if should_skip(key, value) {
                continue;
            }

            if key.trim().is_empty() || value.trim().is_empty() {
                continue;
            }

            request = request.header(key.as_str(), value.as_str());
        }

        request
    }

    async fn map_error_response(
        provider_name: &str,
        response: reqwest::Response,
        default_message: &str,
    ) -> DomainError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = extract_error_message(&body, default_message);

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                DomainError::AuthenticationError(message)
            }
            StatusCode::BAD_REQUEST => DomainError::InvalidData(message),
            _ => DomainError::InternalError(format!(
                "{provider_name} endpoint failed with status {}: {message}",
                status.as_u16()
            )),
        }
    }

    async fn stream_sse_response(
        provider_name: &str,
        response: reqwest::Response,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), DomainError> {
        Self::stream_sse_response_internal(provider_name, response, sender, cancel, |_| {}).await
    }

    async fn stream_sse_response_internal<F>(
        provider_name: &str,
        mut response: reqwest::Response,
        sender: ChatCompletionStreamSender,
        mut cancel: ChatCompletionCancelReceiver,
        mut hook: F,
    ) -> Result<(), DomainError>
    where
        F: FnMut(&[u8]),
    {
        let mut buffer = Vec::<u8>::new();
        let mut accumulator = SseEventAccumulator::default();

        loop {
            if *cancel.borrow() {
                return Ok(());
            }

            let chunk = tokio::select! {
                _ = cancel.changed() => {
                    if *cancel.borrow() {
                        return Ok(());
                    }
                    continue;
                }
                chunk = response.chunk() => {
                    chunk.map_err(|error| DomainError::InternalError(format!(
                        "{provider_name} stream read failed: {error}"
                    )))?
                }
            };

            let Some(chunk) = chunk else {
                break;
            };

            buffer.extend_from_slice(&chunk);
            Self::forward_sse_events(&mut buffer, &mut accumulator, &sender, &mut hook)?;
        }

        if !buffer.is_empty() {
            Self::forward_sse_events(&mut buffer, &mut accumulator, &sender, &mut hook)?;
            Self::forward_sse_line(buffer.as_slice(), &mut accumulator, &sender, &mut hook)?;
            buffer.clear();
        }

        accumulator.finish(&sender, &mut hook)?;
        Ok(())
    }

    fn forward_sse_events<F: FnMut(&[u8])>(
        buffer: &mut Vec<u8>,
        accumulator: &mut SseEventAccumulator,
        sender: &ChatCompletionStreamSender,
        hook: &mut F,
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

            accumulator.on_line(line, sender, hook)?;
            consumed = index + 1;
            line_start = consumed;
        }

        if consumed > 0 {
            buffer.drain(..consumed);
        }

        Ok(())
    }

    fn forward_sse_line<F: FnMut(&[u8])>(
        line: &[u8],
        accumulator: &mut SseEventAccumulator,
        sender: &ChatCompletionStreamSender,
        hook: &mut F,
    ) -> Result<(), DomainError> {
        let mut line = line;
        if line.last().is_some_and(|byte| *byte == b'\r') {
            line = &line[..line.len() - 1];
        }

        accumulator.on_line(line, sender, hook)
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
    if let Some(usage) = value.get("usage").and_then(Value::as_object) {
        if let Some(parsed) = parse_prompt_cache_performance_usage(usage) {
            return Some(parsed);
        }
    }

    if let Some(message_usage) = value
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("usage"))
        .and_then(Value::as_object)
    {
        if let Some(parsed) = parse_prompt_cache_performance_usage(message_usage) {
            return Some(parsed);
        }
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
    value.and_then(|value| {
        value.as_u64().or_else(|| {
            value
                .as_i64()
                .filter(|number| *number >= 0)
                .and_then(|number| u64::try_from(number).ok())
        })
    })
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
            | ChatCompletionSource::SiliconFlow
            | ChatCompletionSource::Zai => openai::list_models(self, config, source_name).await,
            ChatCompletionSource::Cohere => cohere::list_models(self, config).await,
            ChatCompletionSource::NanoGpt => {
                openai::list_models_with_path(self, config, source_name, "/models?detailed=true")
                    .await
            }
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

        match source {
            ChatCompletionSource::OpenAi
            | ChatCompletionSource::OpenRouter
            | ChatCompletionSource::DeepSeek
            | ChatCompletionSource::Groq
            | ChatCompletionSource::Moonshot
            | ChatCompletionSource::NanoGpt
            | ChatCompletionSource::Chutes
            | ChatCompletionSource::SiliconFlow
            | ChatCompletionSource::Zai => {
                openai::generate(self, config, endpoint_path, payload, source_name)
                    .await
                    .map(ChatCompletionRepositoryGenerateResponse::from_body)
            }
            ChatCompletionSource::Custom => {
                if endpoint_path == "/responses" {
                    openai_responses::generate(
                        self,
                        config,
                        endpoint_path,
                        payload,
                        "Custom OpenAI Responses",
                    )
                    .await
                } else if endpoint_path == "/interactions" {
                    gemini_interactions::generate(
                        self,
                        config,
                        endpoint_path,
                        payload,
                        "Custom Gemini Interactions",
                    )
                    .await
                } else if endpoint_path == "/messages" {
                    claude::generate(
                        self,
                        config,
                        endpoint_path,
                        payload,
                        "Custom Claude Messages",
                    )
                    .await
                } else {
                    openai::generate(self, config, endpoint_path, payload, source_name)
                        .await
                        .map(ChatCompletionRepositoryGenerateResponse::from_body)
                }
            }
            ChatCompletionSource::Cohere => cohere::generate(self, config, endpoint_path, payload)
                .await
                .map(ChatCompletionRepositoryGenerateResponse::from_body),
            ChatCompletionSource::Claude => {
                claude::generate(self, config, endpoint_path, payload, source_name).await
            }
            ChatCompletionSource::Makersuite => {
                makersuite::generate(self, config, endpoint_path, payload).await
            }
            ChatCompletionSource::VertexAi => {
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

        match source {
            ChatCompletionSource::OpenAi
            | ChatCompletionSource::OpenRouter
            | ChatCompletionSource::DeepSeek
            | ChatCompletionSource::Groq
            | ChatCompletionSource::Moonshot
            | ChatCompletionSource::NanoGpt
            | ChatCompletionSource::Chutes
            | ChatCompletionSource::SiliconFlow
            | ChatCompletionSource::Zai => {
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
            ChatCompletionSource::Custom => {
                if endpoint_path == "/responses" {
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
                } else if endpoint_path == "/interactions" {
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
                } else if endpoint_path == "/messages" {
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
                } else {
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
            }
            ChatCompletionSource::Cohere => {
                cohere::generate_stream(self, config, endpoint_path, payload, sender, cancel).await
            }
            ChatCompletionSource::Claude => {
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
            ChatCompletionSource::Makersuite => {
                makersuite::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
            }
            ChatCompletionSource::VertexAi => {
                vertexai::generate_stream(self, config, endpoint_path, payload, sender, cancel)
                    .await
            }
        }
    }
}

fn extract_error_message(body: &str, default_message: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return default_message.to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(Value::as_object)
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return message.to_string();
        }

        if let Some(message) = value
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return message.to_string();
        }
    }

    body.to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reqwest::Client;
    use reqwest::header::AUTHORIZATION;
    use tokio::sync::mpsc;

    use crate::domain::repositories::chat_completion_repository::ChatCompletionApiConfig;

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
    fn apply_extra_headers_skips_empty_keys_and_values() {
        let mut headers = HashMap::new();
        headers.insert("x-empty-value".to_string(), "   ".to_string());
        headers.insert("   ".to_string(), "value".to_string());
        headers.insert("x-valid".to_string(), "ok".to_string());

        let request = Client::new().get("https://example.com");
        let request = HttpChatCompletionRepository::apply_extra_headers(request, &headers);
        let request = request.build().expect("request should build");

        assert!(request.headers().get("x-empty-value").is_none());
        assert!(request.headers().get("   ").is_none());
        assert_eq!(
            request
                .headers()
                .get("x-valid")
                .and_then(|value| value.to_str().ok()),
            Some("ok")
        );
    }

    #[test]
    fn apply_openai_auth_prefers_explicit_authorization_header() {
        let config = ChatCompletionApiConfig {
            base_url: "https://example.com/v1".to_string(),
            api_key: "saved-secret".to_string(),
            authorization_header: Some("Bearer override".to_string()),
            extra_headers: HashMap::new(),
            anthropic_beta_header_mode:
                crate::domain::repositories::chat_completion_repository::AnthropicBetaHeaderMode::None,
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
    fn forward_sse_events_extracts_data_payloads() {
        let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
        let mut buffer =
            b"event: message\r\ndata: {\"chunk\":1}\n\n: ping\ndata: [DONE]\n\n".to_vec();

        fn noop(_: &[u8]) {}
        let mut hook = noop;
        let mut accumulator = super::SseEventAccumulator::default();
        let result = HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &sender,
            &mut hook,
        );
        assert!(result.is_ok());

        assert_eq!(receiver.try_recv().ok(), Some("{\"chunk\":1}".to_string()));
        assert_eq!(receiver.try_recv().ok(), Some("[DONE]".to_string()));
        assert!(receiver.try_recv().is_err());
        assert!(buffer.is_empty());
    }

    #[test]
    fn forward_sse_events_keeps_partial_line_in_buffer() {
        let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
        let mut buffer = b"data: {\"chunk\":1}".to_vec();

        fn noop(_: &[u8]) {}
        let mut hook = noop;
        let mut accumulator = super::SseEventAccumulator::default();
        let result = HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &sender,
            &mut hook,
        );
        assert!(result.is_ok());
        assert_eq!(receiver.try_recv().ok(), None);
        assert_eq!(buffer, b"data: {\"chunk\":1}".to_vec());
    }

    #[test]
    fn forward_sse_events_combines_multiline_data_fields() {
        let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
        let mut buffer = b"data: first\ndata: second\n\n".to_vec();

        fn noop(_: &[u8]) {}
        let mut hook = noop;
        let mut accumulator = super::SseEventAccumulator::default();
        HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &sender,
            &mut hook,
        )
        .unwrap();

        assert_eq!(receiver.try_recv().ok(), Some("first\nsecond".to_string()));
        assert!(receiver.try_recv().is_err());
        assert!(buffer.is_empty());
    }

    #[test]
    fn forward_sse_events_can_flush_pending_event_at_end_of_stream() {
        let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
        let mut buffer = b"data: tail\n".to_vec();

        fn noop(_: &[u8]) {}
        let mut hook = noop;
        let mut accumulator = super::SseEventAccumulator::default();
        HttpChatCompletionRepository::forward_sse_events(
            &mut buffer,
            &mut accumulator,
            &sender,
            &mut hook,
        )
        .unwrap();

        // No blank line yet, so no event dispatched.
        assert!(receiver.try_recv().is_err());

        accumulator.finish(&sender, &mut hook).unwrap();

        assert_eq!(receiver.try_recv().ok(), Some("tail".to_string()));
        assert!(receiver.try_recv().is_err());
    }
}
