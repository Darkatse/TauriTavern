use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;

const OPENAI_MESSAGE_TO_TEXT_TOKEN_OFFSET: usize = 1;

/// Converts a raw single-message OpenAI token count into the caller-visible
/// text token count used by prefix budget checks.
pub fn openai_text_token_count(message_token_count: usize) -> usize {
    message_token_count.saturating_sub(OPENAI_MESSAGE_TO_TEXT_TOKEN_OFFSET)
}

/// Converts a caller-visible text limit into the plain-content limit used
/// before adding a tokenizer's single-message wrapper.
pub fn openai_content_token_limit(text_token_limit: usize, message_wrapper_tokens: usize) -> usize {
    text_token_limit
        .saturating_add(OPENAI_MESSAGE_TO_TEXT_TOKEN_OFFSET)
        .saturating_sub(message_wrapper_tokens)
}

#[async_trait]
pub trait TokenizerRepository: Send + Sync {
    async fn ensure_model_ready(&self, model: &str) -> Result<(), DomainError>;

    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError>;

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError>;

    fn count_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError>;

    /// Counts or estimates cumulative system-message prefixes and returns raw
    /// wrapper-inclusive message counts. `stop_at` is applied to those counts and
    /// excludes the single-message wrapper offset.
    fn count_system_message_prefixes(
        &self,
        model: &str,
        base: &str,
        suffixes: &[String],
        stop_at: Option<usize>,
    ) -> Result<Vec<usize>, DomainError> {
        let additional_capacity = suffixes
            .iter()
            .fold(0_usize, |total, suffix| total.saturating_add(suffix.len()));
        let mut content = String::with_capacity(base.len().saturating_add(additional_capacity));
        content.push_str(base);

        let mut token_counts = Vec::with_capacity(suffixes.len());
        for suffix in suffixes {
            content.push_str(suffix);
            let message = serde_json::json!({ "role": "system", "content": content });
            let token_count = self.count_messages(model, &[message])?;
            token_counts.push(token_count);

            if stop_at.is_some_and(|limit| openai_text_token_count(token_count) >= limit) {
                token_counts.resize(suffixes.len(), token_count);
                break;
            }
        }

        Ok(token_counts)
    }
}

#[cfg(test)]
mod tests {
    use super::{openai_content_token_limit, openai_text_token_count};

    #[test]
    fn openai_text_and_content_limits_account_for_the_message_wrapper() {
        assert_eq!(openai_text_token_count(0), 0);
        assert_eq!(openai_text_token_count(1), 0);
        assert_eq!(openai_text_token_count(13), 12);
        assert_eq!(openai_content_token_limit(12, 7), 6);
        assert_eq!(openai_content_token_limit(0, 7), 0);
    }
}
