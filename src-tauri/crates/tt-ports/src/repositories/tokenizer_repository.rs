use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;

#[async_trait]
pub trait TokenizerRepository: Send + Sync {
    async fn ensure_model_ready(&self, model: &str) -> Result<(), DomainError>;

    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError>;

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError>;

    fn count_messages(&self, model: &str, messages: &[Value]) -> Result<usize, DomainError>;

    /// Counts or estimates cumulative text prefixes as content-only messages.
    /// Empty text counts as zero. `stop_at` uses these same caller-visible counts;
    /// after reaching it, remaining entries repeat the terminal count.
    fn count_text_prefixes(
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
            let token_count = if content.is_empty() {
                0
            } else {
                self.count_messages(model, &[serde_json::json!({ "content": content })])?
            };
            token_counts.push(token_count);

            if stop_at.is_some_and(|limit| token_count >= limit) {
                token_counts.resize(suffixes.len(), token_count);
                break;
            }
        }

        Ok(token_counts)
    }
}
