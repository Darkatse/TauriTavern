use async_trait::async_trait;
pub use tt_contracts::provider_metadata::{
    ModelProvider, NanoGptCredits, NanoGptModelProviders, NanoGptSubscriptionCredits,
    NanoGptSubscriptionLimits, NanoGptSubscriptionPeriod, NanoGptUsageBucket, OpenRouterCredits,
};

use tt_domain::errors::DomainError;

#[async_trait]
pub trait ProviderMetadataRepository: Send + Sync {
    async fn openrouter_providers(&self) -> Result<Vec<ModelProvider>, DomainError>;

    async fn openrouter_model_providers(
        &self,
        model: &str,
    ) -> Result<Vec<ModelProvider>, DomainError>;

    async fn openrouter_credits(&self, api_key: &str) -> Result<OpenRouterCredits, DomainError>;

    async fn exchange_openrouter_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<String, DomainError>;

    async fn nanogpt_model_providers(
        &self,
        model: &str,
    ) -> Result<NanoGptModelProviders, DomainError>;

    async fn nanogpt_credits(&self, api_key: &str) -> Result<NanoGptCredits, DomainError>;
}
