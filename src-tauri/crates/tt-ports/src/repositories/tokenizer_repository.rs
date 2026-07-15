use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;

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

        if stop_at.is_some_and(|limit| token_count.saturating_sub(1) >= limit) {
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
