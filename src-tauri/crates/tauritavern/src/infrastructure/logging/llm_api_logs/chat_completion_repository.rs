use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::readable::{
    StreamReadableCollector, format_endpoint, format_request_readable, format_response_readable,
    stream_readable_source,
};
use super::recording::GenerationLog;
use super::store::LlmApiLogStore;
use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionRepository, ChatCompletionRepositoryGenerateResponse,
    ChatCompletionSource, ChatCompletionStreamDelta,
};

pub struct LoggingChatCompletionRepository {
    inner: Arc<dyn ChatCompletionRepository>,
    store: Arc<LlmApiLogStore>,
}

impl LoggingChatCompletionRepository {
    pub fn new(inner: Arc<dyn ChatCompletionRepository>, store: Arc<LlmApiLogStore>) -> Self {
        Self { inner, store }
    }

    fn start_log<'a>(
        &'a self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &'a Value,
        stream: bool,
    ) -> GenerationLog<'a> {
        GenerationLog::start(
            &self.store,
            source.key().to_string(),
            format_endpoint(&config.base_url, endpoint_path),
            payload,
            stream,
            |payload| format_request_readable(source, payload),
        )
    }
}

#[async_trait]
impl ChatCompletionRepository for LoggingChatCompletionRepository {
    async fn list_models(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
    ) -> Result<Value, DomainError> {
        self.inner.list_models(source, config).await
    }

    async fn generate(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
    ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
        let log = self.start_log(source, config, endpoint_path, payload, false);
        let result = self
            .inner
            .generate(source, config, endpoint_path, payload)
            .await;
        log.finish_json(
            result.as_ref().map(|response| &response.body),
            format_response_readable,
        )
        .await;
        result
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
        let readable = StreamReadableCollector::new(stream_readable_source(source, endpoint_path));
        self.start_log(source, config, endpoint_path, payload, true)
            .record_stream(sender, readable, |sender| {
                self.inner
                    .generate_stream(source, config, endpoint_path, payload, sender, cancel)
            })
            .await
    }

    async fn generate_with_deltas(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        on_delta: &mut (dyn FnMut(ChatCompletionStreamDelta) + Send),
    ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
        let log = self.start_log(source, config, endpoint_path, payload, true);
        let result = self
            .inner
            .generate_with_deltas(source, config, endpoint_path, payload, on_delta)
            .await;
        log.finish_json(
            result.as_ref().map(|response| &response.body),
            format_response_readable,
        )
        .await;
        result
    }

    async fn close_provider_session(&self, session_id: &str) {
        self.inner.close_provider_session(session_id).await;
    }
}
