use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelProvidersRequestDto {
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiliconFlowEmbeddingModelsRequestDto {
    #[serde(default)]
    pub siliconflow_endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkersAiModelsRequestDto {
    #[serde(default)]
    pub workers_ai_account_id: String,
}
