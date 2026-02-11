use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatCompletionSource {
    OpenAi,
    Custom,
    Claude,
    Makersuite,
}

impl ChatCompletionSource {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "" | "openai" => Some(Self::OpenAi),
            "custom" => Some(Self::Custom),
            "claude" => Some(Self::Claude),
            "makersuite" | "gemini" | "google" => Some(Self::Makersuite),
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
