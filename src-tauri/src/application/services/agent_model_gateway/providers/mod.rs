use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::application::services::chat_completion_service::exchange::ChatCompletionProviderFormat;
use crate::domain::models::agent::AgentModelRequest;

mod claude;
mod gemini;
mod openai;
mod responses;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentProviderAdapter {
    OpenAiCompatible,
    OpenAiResponses,
    ClaudeMessages,
    Gemini,
    GeminiInteractions,
}

impl AgentProviderAdapter {
    pub(super) const fn from_format(format: ChatCompletionProviderFormat) -> Self {
        match format {
            ChatCompletionProviderFormat::OpenAiCompatible => Self::OpenAiCompatible,
            ChatCompletionProviderFormat::OpenAiResponses => Self::OpenAiResponses,
            ChatCompletionProviderFormat::ClaudeMessages => Self::ClaudeMessages,
            ChatCompletionProviderFormat::Gemini => Self::Gemini,
            ChatCompletionProviderFormat::GeminiInteractions => Self::GeminiInteractions,
        }
    }

    pub(super) const fn format(self) -> ChatCompletionProviderFormat {
        match self {
            Self::OpenAiCompatible => ChatCompletionProviderFormat::OpenAiCompatible,
            Self::OpenAiResponses => ChatCompletionProviderFormat::OpenAiResponses,
            Self::ClaudeMessages => ChatCompletionProviderFormat::ClaudeMessages,
            Self::Gemini => ChatCompletionProviderFormat::Gemini,
            Self::GeminiInteractions => ChatCompletionProviderFormat::GeminiInteractions,
        }
    }

    pub(super) fn messages_for_request<'a>(
        self,
        request: &'a AgentModelRequest,
    ) -> Result<Vec<&'a crate::domain::models::agent::AgentModelMessage>, ApplicationError> {
        match self {
            Self::OpenAiResponses => responses::messages_for_request(request),
            _ => Ok(request.messages.iter().collect()),
        }
    }

    pub(super) fn apply_payload_overrides(
        self,
        payload: &mut Map<String, Value>,
        request: &AgentModelRequest,
    ) -> Result<(), ApplicationError> {
        match self {
            Self::OpenAiResponses => responses::apply_payload_overrides(payload, request),
            _ => Ok(()),
        }
    }

    pub(super) fn finalize_payload(self, payload: &mut Map<String, Value>) {
        if self == Self::OpenAiResponses {
            responses::ensure_reasoning_include(payload);
        }
    }

    pub(super) const fn native_provider(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => openai::NATIVE_PROVIDER,
            Self::OpenAiResponses => responses::NATIVE_PROVIDER,
            Self::ClaudeMessages => claude::NATIVE_PROVIDER,
            Self::Gemini => gemini::GEMINI_NATIVE_PROVIDER,
            Self::GeminiInteractions => gemini::INTERACTIONS_NATIVE_PROVIDER,
        }
    }

    pub(super) const fn schema_keys_to_remove(self) -> &'static [&'static str] {
        match self {
            Self::OpenAiCompatible | Self::OpenAiResponses => openai::SCHEMA_KEYS_TO_REMOVE,
            Self::ClaudeMessages => claude::SCHEMA_KEYS_TO_REMOVE,
            Self::Gemini | Self::GeminiInteractions => gemini::SCHEMA_KEYS_TO_REMOVE,
        }
    }
}
