use async_trait::async_trait;
use serde_json::Value;
pub use tt_contracts::provider_metadata::{
    NanoGptCredits, NanoGptModelProviders, NanoGptSubscriptionCredits, NanoGptSubscriptionLimits,
    NanoGptSubscriptionPeriod, NanoGptUsageBucket, OpenRouterCredits,
};

use tt_domain::errors::DomainError;
use tt_domain::models::siliconflow::SiliconFlowEndpoint;
use tt_domain::models::workers_ai::WorkersAiAccount;

#[async_trait]
pub trait ProviderMetadataRepository: Send + Sync {
    async fn openrouter_model_providers(&self, model: &str) -> Result<Vec<String>, DomainError>;

    async fn openrouter_credits(&self, api_key: &str) -> Result<OpenRouterCredits, DomainError>;

    async fn nanogpt_model_providers(
        &self,
        model: &str,
    ) -> Result<NanoGptModelProviders, DomainError>;

    async fn nanogpt_credits(&self, api_key: &str) -> Result<NanoGptCredits, DomainError>;

    async fn siliconflow_embedding_models(
        &self,
        api_key: &str,
        endpoint: SiliconFlowEndpoint,
    ) -> Result<Vec<Value>, DomainError>;

    async fn workers_ai_embedding_models(
        &self,
        api_key: &str,
        account: &WorkersAiAccount,
    ) -> Result<Vec<Value>, DomainError>;

    async fn workers_ai_multimodal_models(
        &self,
        api_key: &str,
        account: &WorkersAiAccount,
    ) -> Result<Vec<String>, DomainError>;
}
