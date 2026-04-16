use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatCompletionSource {
    OpenAi,
    OpenRouter,
    Custom,
    Claude,
    Makersuite,
    VertexAi,
    DeepSeek,
    Cohere,
    Groq,
    Moonshot,
    NanoGpt,
    Chutes,
    SiliconFlow,
    Zai,
}

impl ChatCompletionSource {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "" | "openai" => Some(Self::OpenAi),
            "openrouter" | "open-router" => Some(Self::OpenRouter),
            "custom" => Some(Self::Custom),
            "claude" => Some(Self::Claude),
            "makersuite" | "gemini" | "google" => Some(Self::Makersuite),
            "vertexai" | "vertex-ai" | "vertex ai" => Some(Self::VertexAi),
            "deepseek" => Some(Self::DeepSeek),
            "cohere" => Some(Self::Cohere),
            "groq" => Some(Self::Groq),
            "moonshot" | "moonshot ai" => Some(Self::Moonshot),
            "nanogpt" | "nano-gpt" | "nano gpt" => Some(Self::NanoGpt),
            "chutes" => Some(Self::Chutes),
            "siliconflow" | "silicon flow" => Some(Self::SiliconFlow),
            "zai" | "z.ai" | "glm" => Some(Self::Zai),
            _ => None,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
            Self::Custom => "custom",
            Self::Claude => "claude",
            Self::Makersuite => "makersuite",
            Self::VertexAi => "vertexai",
            Self::DeepSeek => "deepseek",
            Self::Cohere => "cohere",
            Self::Groq => "groq",
            Self::Moonshot => "moonshot",
            Self::NanoGpt => "nanogpt",
            Self::Chutes => "chutes",
            Self::SiliconFlow => "siliconflow",
            Self::Zai => "zai",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::OpenRouter => "OpenRouter",
            Self::Custom => "Custom OpenAI",
            Self::Claude => "Claude",
            Self::Makersuite => "Google Gemini",
            Self::VertexAi => "Google Vertex AI",
            Self::DeepSeek => "DeepSeek",
            Self::Cohere => "Cohere",
            Self::Groq => "Groq",
            Self::Moonshot => "Moonshot AI",
            Self::NanoGpt => "NanoGPT",
            Self::Chutes => "Chutes",
            Self::SiliconFlow => "SiliconFlow",
            Self::Zai => "Z.AI (GLM)",
        }
    }
}

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
    pub extra_headers: HashMap<String, String>,
    pub anthropic_beta_header_mode: AnthropicBetaHeaderMode,
}

pub type ChatCompletionStreamSender = UnboundedSender<String>;
pub type ChatCompletionCancelReceiver = watch::Receiver<bool>;

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
    ) -> Result<Value, DomainError>;

    async fn generate_stream(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), DomainError>;
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
            ChatCompletionSource::parse("zai"),
            Some(ChatCompletionSource::Zai)
        );
        assert_eq!(
            ChatCompletionSource::parse("vertexai"),
            Some(ChatCompletionSource::VertexAi)
        );
    }
}
