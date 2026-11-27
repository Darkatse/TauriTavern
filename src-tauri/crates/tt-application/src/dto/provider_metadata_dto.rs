use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelProvidersRequestDto {
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenRouterCodeExchangeRequestDto {
    pub code: String,
    pub code_verifier: String,
}
