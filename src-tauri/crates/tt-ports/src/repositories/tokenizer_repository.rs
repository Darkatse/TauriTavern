use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;

/// Estimates, and whether the fallback tokenizer made them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Estimated<T> {
    pub value: T,
    /// The model's own tokenizer could not be downloaded, so the fallback tokenizer estimated in
    /// its place. Estimates for the model may differ once it downloads.
    pub fallback: bool,
}

/// Tokenizers by model name. Counts are estimates; `encode` and `decode` are exact.
///
/// The other methods use the tokenizer that `ensure_model_ready` prepared for the name.
#[async_trait]
pub trait TokenizerRepository: Send + Sync {
    /// Prepares the tokenizer of the model the name resolves to. A name no tokenizer is known for
    /// uses the o200k tokenizer.
    async fn ensure_model_ready(&self, model: &str) -> Result<(), DomainError>;

    fn encode(&self, model: &str, text: &str) -> Result<Vec<u32>, DomainError>;

    fn decode(&self, model: &str, token_ids: &[u32]) -> Result<String, DomainError>;

    /// Estimates each request, a list of OpenAI-style messages, with the tokens that frame each
    /// message and the reply.
    fn estimate_messages(
        &self,
        model: &str,
        requests: &[&[Value]],
    ) -> Result<Estimated<Vec<usize>>, DomainError>;

    /// Estimates plain text, without message framing.
    fn estimate_text(&self, model: &str, text: &str) -> Result<Estimated<usize>, DomainError>;

    /// Estimates `base` followed by each prefix of `suffixes`, as a message of only that content.
    /// Empty text is zero. Once an estimate reaches `stop_at`, the remaining entries repeat it.
    fn estimate_text_prefixes(
        &self,
        model: &str,
        base: &str,
        suffixes: &[String],
        stop_at: Option<usize>,
    ) -> Result<Estimated<Vec<usize>>, DomainError>;
}
