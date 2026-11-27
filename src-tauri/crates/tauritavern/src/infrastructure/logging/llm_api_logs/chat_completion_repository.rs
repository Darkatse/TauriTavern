use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::recording::GenerationLog;
use super::request::LoggedRequest;
use super::store::LlmApiLogStore;
use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionRepository, ChatCompletionRepositoryGenerateResponse,
    ChatCompletionSource, ChatCompletionStreamDelta, GenerationCall, WireRequest,
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
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &'a Value,
        call: GenerationCall,
    ) -> GenerationLog<'a> {
        // Sending reports construction errors; still log the original payload.
        let (body, http) =
            match self
                .inner
                .describe_request(source, config, endpoint_path, payload, call)
            {
                Ok(request) => (request.body, Some(request.http)),
                Err(_) => (Cow::Borrowed(payload), None),
            };
        GenerationLog::start(
            Arc::clone(&self.store),
            LoggedRequest::new(
                source.key().to_string(),
                &config.base_url,
                endpoint_path,
                payload,
                body,
                http,
            ),
            call.is_stream(),
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
        let log = self.start_log(
            source,
            config,
            endpoint_path,
            payload,
            GenerationCall::Generate,
        );
        let result = self
            .inner
            .generate(source, config, endpoint_path, payload)
            .await;
        log.finish_json(result.as_ref().map(|response| &response.body));
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
        self.start_log(
            source,
            config,
            endpoint_path,
            payload,
            GenerationCall::GenerateStream,
        )
        .record_stream(sender, |sender| {
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
        let log = self.start_log(
            source,
            config,
            endpoint_path,
            payload,
            GenerationCall::GenerateWithDeltas,
        );
        let result = self
            .inner
            .generate_with_deltas(source, config, endpoint_path, payload, on_delta)
            .await;
        log.finish_json(result.as_ref().map(|response| &response.body));
        result
    }

    async fn close_provider_session(&self, session_id: &str) {
        self.inner.close_provider_session(session_id).await;
    }

    fn describe_request<'a>(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &'a Value,
        call: GenerationCall,
    ) -> Result<WireRequest<'a>, DomainError> {
        self.inner
            .describe_request(source, config, endpoint_path, payload, call)
    }
}
