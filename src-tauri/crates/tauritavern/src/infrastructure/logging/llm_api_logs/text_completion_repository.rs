use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::recording::GenerationLog;
use super::request::LoggedRequest;
use super::store::LlmApiLogStore;
use super::types::TEXT_COMPLETION_SOURCE_PREFIX;
use tt_contracts::observability::RequestDescriptionDto;
use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::text_completion_repository::{
    TextCompletionEndpoint, TextCompletionRepository, TextCompletionStreamFormat,
};

/// Records generations only.
pub struct LoggingTextCompletionRepository {
    inner: Arc<dyn TextCompletionRepository>,
    store: Arc<LlmApiLogStore>,
}

impl LoggingTextCompletionRepository {
    pub fn new(inner: Arc<dyn TextCompletionRepository>, store: Arc<LlmApiLogStore>) -> Self {
        Self { inner, store }
    }

    fn start_log<'a>(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &'a Value,
        stream: Option<TextCompletionStreamFormat>,
    ) -> GenerationLog<'a> {
        // Sending reports construction errors.
        let http = self.inner.describe_request(endpoint, path, stream).ok();
        GenerationLog::start(
            Arc::clone(&self.store),
            LoggedRequest::new(
                format!("{TEXT_COMPLETION_SOURCE_PREFIX}{}", endpoint.api_type.key()),
                &endpoint.base_url,
                path,
                body,
                Cow::Borrowed(body),
                http,
            ),
            stream.is_some(),
        )
    }
}

#[async_trait]
impl TextCompletionRepository for LoggingTextCompletionRepository {
    async fn get(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
    ) -> Result<Value, DomainError> {
        self.inner.get(endpoint, path).await
    }

    async fn post(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError> {
        self.inner.post(endpoint, path, body).await
    }

    async fn download(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError> {
        self.inner.download(endpoint, path, body).await
    }

    async fn generate(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError> {
        let log = self.start_log(endpoint, path, body, None);
        let result = self.inner.generate(endpoint, path, body).await;
        log.finish_json(result.as_ref());
        result
    }

    async fn generate_stream(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
        format: TextCompletionStreamFormat,
        sender: GenerationStreamSender,
        cancel: GenerationCancelReceiver,
    ) -> Result<(), DomainError> {
        self.start_log(endpoint, path, body, Some(format))
            .record_stream(sender, |sender| {
                self.inner
                    .generate_stream(endpoint, path, body, format, sender, cancel)
            })
            .await
    }

    fn describe_request(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        stream: Option<TextCompletionStreamFormat>,
    ) -> Result<RequestDescriptionDto, DomainError> {
        self.inner.describe_request(endpoint, path, stream)
    }
}
