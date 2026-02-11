use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatCompletionSource {
    OpenAi,
    OpenRouter,
    Custom,
    Claude,
    Makersuite,
    DeepSeek,
    Moonshot,
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
            "deepseek" => Some(Self::DeepSeek),
            "moonshot" | "moonshot ai" => Some(Self::Moonshot),
            "siliconflow" | "silicon flow" => Some(Self::SiliconFlow),
            "zai" | "z.ai" | "glm" => Some(Self::Zai),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatCompletionApiConfig {
    pub base_url: String,
    pub api_key: String,
    pub extra_headers: HashMap<String, String>,
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
    ) -> Result<Value, DomainError>;
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
            ChatCompletionSource::parse("openrouter"),
            Some(ChatCompletionSource::OpenRouter)
        );
        assert_eq!(
            ChatCompletionSource::parse("moonshot"),
            Some(ChatCompletionSource::Moonshot)
        );
        assert_eq!(
            ChatCompletionSource::parse("siliconflow"),
            Some(ChatCompletionSource::SiliconFlow)
        );
        assert_eq!(
            ChatCompletionSource::parse("zai"),
            Some(ChatCompletionSource::Zai)
        );
    }
}
