use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::{mpsc::UnboundedSender, watch};

use tt_domain::errors::DomainError;
pub use tt_domain::models::chat_completion_source::ChatCompletionSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnthropicBetaHeaderMode {
    #[default]
    None,
    PromptCachingOnly,
    ClaudeDefaults,
}

#[derive(Debug, Clone)]
pub struct ChatCompletionApiConfig {
    pub base_url: String,
    pub api_key: String,
    pub authorization_header: Option<String>,
    pub vertexai_service_account_json: Option<String>,
    pub extra_headers: HashMap<String, String>,
    pub additional_headers: HashMap<String, String>,
    pub anthropic_beta_header_mode: AnthropicBetaHeaderMode,
    /// Optional dotted JSON path (e.g. `output.message.content.0.text`) used by
    /// the AWS Bedrock custom-template escape hatch to lift the assistant text
    /// out of an arbitrary non-stream response body. When set, the
    /// infrastructure layer bypasses provider-specific normalizers and
    /// extracts text from this path instead.
    pub aws_bedrock_custom_response_path: Option<String>,
    /// Same as [`aws_bedrock_custom_response_path`] but applied to each
    /// streaming chunk JSON. Empty / missing chunks are silently dropped so
    /// terminal sentinel events don't surface as blank deltas.
    pub aws_bedrock_custom_stream_path: Option<String>,
}

pub type ChatCompletionStreamSender = UnboundedSender<String>;
pub type ChatCompletionCancelReceiver = watch::Receiver<bool>;
pub const CHAT_COMPLETION_PROVIDER_STATE_FIELD: &str = "_tauritavern_provider_state";

#[derive(Debug, Clone, Default)]
pub struct ChatCompletionNormalizationReport {
    pub synthetic_tool_call_ids: Vec<String>,
}

impl ChatCompletionNormalizationReport {
    pub fn synthetic_tool_call_ids(&self) -> &[String] {
        &self.synthetic_tool_call_ids
    }

    pub fn record_synthetic_tool_call_id(&mut self, id: impl Into<String>) {
        self.synthetic_tool_call_ids.push(id.into());
    }
}

#[derive(Debug, Clone)]
pub struct ChatCompletionRepositoryGenerateResponse {
    pub body: Value,
    pub normalization_report: ChatCompletionNormalizationReport,
}

impl ChatCompletionRepositoryGenerateResponse {
    pub fn new(body: Value, normalization_report: ChatCompletionNormalizationReport) -> Self {
        Self {
            body,
            normalization_report,
        }
    }

    pub fn from_body(body: Value) -> Self {
        Self::new(body, ChatCompletionNormalizationReport::default())
    }
}

#[async_trait]
pub trait ChatCompletionRepository: Send + Sync {
    async fn list_models(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
    ) -> Result<Value, DomainError>;

    async fn generate(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
    ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError>;

    async fn generate_stream(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), DomainError>;

    async fn close_provider_session(&self, session_id: &str);
}

#[cfg(test)]
mod tests {
    use super::ChatCompletionSource;

    #[test]
    fn parse_new_openai_compatible_sources() {
        assert_eq!(
            ChatCompletionSource::parse("deepseek"),
            Some(ChatCompletionSource::DeepSeek)
        );
        assert_eq!(
            ChatCompletionSource::parse("cohere"),
            Some(ChatCompletionSource::Cohere)
        );
        assert_eq!(
            ChatCompletionSource::parse("groq"),
            Some(ChatCompletionSource::Groq)
        );
        assert_eq!(
            ChatCompletionSource::parse("openrouter"),
            Some(ChatCompletionSource::OpenRouter)
        );
        assert_eq!(
            ChatCompletionSource::parse("moonshot"),
            Some(ChatCompletionSource::Moonshot)
        );
        assert_eq!(
            ChatCompletionSource::parse("nanogpt"),
            Some(ChatCompletionSource::NanoGpt)
        );
        assert_eq!(
            ChatCompletionSource::parse("chutes"),
            Some(ChatCompletionSource::Chutes)
        );
        assert_eq!(
            ChatCompletionSource::parse("siliconflow"),
            Some(ChatCompletionSource::SiliconFlow)
        );
        assert_eq!(
            ChatCompletionSource::parse("workers_ai"),
            Some(ChatCompletionSource::WorkersAi)
        );
        assert_eq!(
            ChatCompletionSource::parse("zai"),
            Some(ChatCompletionSource::Zai)
        );
        assert_eq!(
            ChatCompletionSource::parse("minimax"),
            Some(ChatCompletionSource::MiniMax)
        );
        assert_eq!(
            ChatCompletionSource::parse("vertexai"),
            Some(ChatCompletionSource::VertexAi)
        );
        assert_eq!(
            ChatCompletionSource::parse("aws_bedrock"),
            Some(ChatCompletionSource::AwsBedrock)
        );
        assert_eq!(
            ChatCompletionSource::parse("bedrock"),
            Some(ChatCompletionSource::AwsBedrock)
        );
    }
}
