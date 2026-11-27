use serde::Serialize;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelProvider {
    /// Routing ID sent to the provider API, independent of the display label.
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NanoGptModelProviders {
    /// A selectable model can still have no available providers.
    pub supports_provider_selection: bool,
    pub providers: Vec<ModelProvider>,
}
