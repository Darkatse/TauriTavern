use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiliconFlowEndpoint {
    Global,
    China,
}

impl SiliconFlowEndpoint {
    pub fn parse_frontend(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "global" | "com" | "https://api.siliconflow.com/v1" => Ok(Self::Global),
            "cn" | "china" | "https://api.siliconflow.cn/v1" => Ok(Self::China),
            other => Err(format!("Unsupported SiliconFlow endpoint: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRouterCredits {
    pub remaining: f64,
    pub total_credits: f64,
    pub total_usage: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NanoGptUsageBucket {
    pub used: f64,
    pub remaining: f64,
    pub percent_used: f64,
    pub reset_at: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NanoGptSubscriptionPeriod {
    pub current_period_end: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NanoGptSubscriptionLimits {
    pub weekly_input_tokens: f64,
    pub daily_input_tokens: f64,
    pub daily_images: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NanoGptSubscriptionCredits {
    pub active: bool,
    pub state: String,
    pub allow_overage: bool,
    pub period: NanoGptSubscriptionPeriod,
    pub limits: NanoGptSubscriptionLimits,
    #[serde(rename = "weekly_tokens", skip_serializing_if = "Option::is_none")]
    pub weekly_tokens: Option<NanoGptUsageBucket>,
    #[serde(rename = "daily_tokens", skip_serializing_if = "Option::is_none")]
    pub daily_tokens: Option<NanoGptUsageBucket>,
    #[serde(rename = "daily_images", skip_serializing_if = "Option::is_none")]
    pub daily_images: Option<NanoGptUsageBucket>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NanoGptCredits {
    pub usd_balance: f64,
    pub nano_balance: f64,
    pub subscription: Option<NanoGptSubscriptionCredits>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NanoGptModelProviders {
    pub supports_provider_selection: bool,
    pub providers: Vec<String>,
}

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
        account_id: &str,
    ) -> Result<Vec<Value>, DomainError>;

    async fn workers_ai_text_generation_models(
        &self,
        api_key: &str,
        account_id: &str,
    ) -> Result<Vec<Value>, DomainError>;

    async fn workers_ai_multimodal_models(
        &self,
        api_key: &str,
        account_id: &str,
    ) -> Result<Vec<String>, DomainError>;
}
