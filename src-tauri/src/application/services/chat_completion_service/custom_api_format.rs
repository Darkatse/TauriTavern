use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

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
    use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

    #[test]
    fn empty_value_defaults_to_openai_compat() {
        assert_eq!(
            CustomApiFormat::parse("").expect("format should parse"),
            CustomApiFormat::OpenAiCompat
        );
    }

    #[test]
    fn invalid_value_fails_fast() {
        let error = CustomApiFormat::parse("invalid").expect_err("format should fail");
        assert!(
            error
                .to_string()
                .contains("Unsupported custom_api_format: invalid")
        );
    }

    #[test]
    fn claude_messages_uses_claude_model_list_transport() {
        assert_eq!(
            CustomApiFormat::ClaudeMessages.model_list_source(),
            ChatCompletionSource::Claude
        );
    }

    #[test]
    fn gemini_interactions_uses_gemini_model_list_transport() {
        assert_eq!(
            CustomApiFormat::GeminiInteractions.model_list_source(),
            ChatCompletionSource::Makersuite
        );
    }
}
