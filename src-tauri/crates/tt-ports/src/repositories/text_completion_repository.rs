use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;

use crate::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
pub use tt_domain::models::text_completion_type::TextCompletionType;

/// A text completion server and the headers every request to it carries.
#[derive(Debug, Clone)]
pub struct TextCompletionEndpoint {
    pub api_type: TextCompletionType,
    pub base_url: String,
    pub headers: Vec<(String, String)>,
}

/// The wire format of a streamed generation response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCompletionStreamFormat {
    /// Server-sent events whose `data` payloads are passed on unchanged.
    Sse,
    /// Ollama's newline-delimited JSON, passed on as `{ choices: [{ text, thinking }] }` events
    /// that end with `[DONE]`.
    OllamaNdjson,
}

/// Requests to a user-configured text completion server. Paths are relative to the endpoint's
/// base URL and bodies are JSON.
#[async_trait]
pub trait TextCompletionRepository: Send + Sync {
    /// Reads server metadata, such as the model list.
    async fn get(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
    ) -> Result<Value, DomainError>;

    /// Sends a short request.
    async fn post(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError>;

    /// Asks the server to download a model. It answers only when the download ends, so the request
    /// has no time limit.
    async fn download(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError>;

    async fn generate(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
    ) -> Result<Value, DomainError>;

    /// Sends each event of the response to `sender` until the response ends or `cancel` turns
    /// `true`; a provider error inside the stream fails it.
    async fn generate_stream(
        &self,
        endpoint: &TextCompletionEndpoint,
        path: &str,
        body: &Value,
        format: TextCompletionStreamFormat,
        sender: GenerationStreamSender,
        cancel: GenerationCancelReceiver,
    ) -> Result<(), DomainError>;
}
