use crate::errors::ApplicationError;
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum CustomApiFormat {
    #[default]
    OpenAiCompat,
    OpenAiResponses,
    ClaudeMessages,
    GeminiInteractions,
}

impl CustomApiFormat {
    pub(super) fn parse(raw: &str) -> Result<Self, ApplicationError> {
        match raw.trim() {
            "" | "openai_compat" => Ok(Self::OpenAiCompat),
            "openai_responses" => Ok(Self::OpenAiResponses),
            "claude_messages" => Ok(Self::ClaudeMessages),
            "gemini_interactions" => Ok(Self::GeminiInteractions),
            other => Err(ApplicationError::ValidationError(format!(
                "Unsupported custom_api_format: {other}"
            ))),
        }
    }

    pub(super) fn model_list_source(self) -> ChatCompletionSource {
        match self {
            Self::OpenAiCompat | Self::OpenAiResponses => ChatCompletionSource::Custom,
            Self::ClaudeMessages => ChatCompletionSource::Claude,
            Self::GeminiInteractions => ChatCompletionSource::Makersuite,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CustomApiFormat;

    #[test]
    fn invalid_value_fails_fast() {
        let error = CustomApiFormat::parse("invalid").expect_err("format should fail");
        assert!(
            error
                .to_string()
                .contains("Unsupported custom_api_format: invalid")
        );
    }
}
