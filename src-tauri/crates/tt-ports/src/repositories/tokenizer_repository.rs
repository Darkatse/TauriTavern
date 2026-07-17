use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;

const OPENAI_MESSAGE_TO_TEXT_TOKEN_OFFSET: usize = 1;

/// Converts a raw single-message OpenAI token count into the caller-visible
/// text token count used by prefix budget checks.
pub fn openai_text_token_count(message_token_count: usize) -> usize {
    message_token_count.saturating_sub(OPENAI_MESSAGE_TO_TEXT_TOKEN_OFFSET)
}

/// Returns whether a raw single-message count has reached the caller-visible
/// text token threshold supplied through `stop_at`.
pub fn has_reached_openai_text_token_limit(
    message_token_count: usize,
    stop_at: Option<usize>,
) -> bool {
    stop_at.is_some_and(|limit| openai_text_token_count(message_token_count) >= limit)
}

pub fn count_system_message_prefixes_default<T: TokenizerRepository + ?Sized>(
    repository: &T,
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
        let token_count = repository.count_messages(model, &[message])?;
        token_counts.push(token_count);

        if has_reached_openai_text_token_limit(token_count, stop_at) {
            token_counts.resize(suffixes.len(), token_count);
            break;
        }
    }

    Ok(token_counts)
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
        count_system_message_prefixes_default(self, model, base, suffixes, stop_at)
    }
}

#[cfg(test)]
mod tests {
    use super::{has_reached_openai_text_token_limit, openai_text_token_count};

    #[test]
    fn openai_text_token_threshold_excludes_the_message_wrapper_offset() {
        assert_eq!(openai_text_token_count(0), 0);
        assert_eq!(openai_text_token_count(1), 0);
        assert_eq!(openai_text_token_count(13), 12);
        assert!(has_reached_openai_text_token_limit(13, Some(12)));
        assert!(!has_reached_openai_text_token_limit(12, Some(12)));
        assert!(!has_reached_openai_text_token_limit(13, None));
    }
}
