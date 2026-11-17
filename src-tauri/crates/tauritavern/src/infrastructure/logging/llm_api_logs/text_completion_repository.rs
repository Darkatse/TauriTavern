use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::readable::{
    CompletionStreamReadable, format_completion, format_endpoint, format_prompt,
};
use super::recording::GenerationLog;
use super::store::LlmApiLogStore;
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
        &'a self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &'a Value,
        stream: bool,
    ) -> GenerationLog<'a> {
        GenerationLog::start(
            &self.store,
            format!("text_completion:{}", endpoint.api_type.key()),
            format_endpoint(&endpoint.base_url, path),
            body,
            stream,
            format_prompt,
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
        let log = self.start_log(endpoint, path, body, false);
        let result = self.inner.generate(endpoint, path, body).await;
        log.finish_json(result.as_ref(), format_completion).await;
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
        self.start_log(endpoint, path, body, true)
            .record_stream(sender, CompletionStreamReadable::default(), |sender| {
                self.inner
                    .generate_stream(endpoint, path, body, format, sender, cancel)
            })
            .await
    }
}
