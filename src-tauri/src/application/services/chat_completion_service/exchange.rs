use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionNormalizationReport, ChatCompletionSource,
};

use super::custom_api_format::CustomApiFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatCompletionProviderFormat {
    OpenAiCompatible,
    OpenAiResponses,
    ClaudeMessages,
    Gemini,
    GeminiInteractions,
}

impl ChatCompletionProviderFormat {
    pub(crate) fn from_payload(
        source: ChatCompletionSource,
        payload: &Map<String, Value>,
    ) -> Result<Self, ApplicationError> {
        if source == ChatCompletionSource::Custom {
            let format = CustomApiFormat::parse(
                payload
                    .get("custom_api_format")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )?;
            return Ok(match format {
                CustomApiFormat::OpenAiCompat => Self::OpenAiCompatible,
                CustomApiFormat::OpenAiResponses => Self::OpenAiResponses,
                CustomApiFormat::ClaudeMessages => Self::ClaudeMessages,
                CustomApiFormat::GeminiInteractions => Self::GeminiInteractions,
            });
        }

        Ok(match source {
            ChatCompletionSource::Claude => Self::ClaudeMessages,
            ChatCompletionSource::Makersuite | ChatCompletionSource::VertexAi => Self::Gemini,
            _ => Self::OpenAiCompatible,
        })
    }

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::OpenAiResponses => "openai_responses",
            Self::ClaudeMessages => "claude_messages",
            Self::Gemini => "gemini",
            Self::GeminiInteractions => "gemini_interactions",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NormalizedChatCompletionResponse {
    raw: Value,
    assistant_message: Map<String, Value>,
}

impl NormalizedChatCompletionResponse {
    pub(crate) fn from_value(raw: Value) -> Result<Self, ApplicationError> {
        let assistant_message = raw
            .pointer("/choices/0/message")
            .or_else(|| raw.pointer("/message"))
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| {
                ApplicationError::ValidationError(
                    "chat_completion.invalid_response: response is missing assistant message"
                        .to_string(),
                )
            })?;

        Ok(Self {
            raw,
            assistant_message,
        })
    }

    pub(crate) fn raw(&self) -> &Value {
        &self.raw
    }

    pub(crate) fn assistant_message(&self) -> &Map<String, Value> {
        &self.assistant_message
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChatCompletionExchange {
    pub source: ChatCompletionSource,
    pub provider_format: ChatCompletionProviderFormat,
    pub normalized_response: NormalizedChatCompletionResponse,
    pub normalization_report: ChatCompletionNormalizationReport,
}
