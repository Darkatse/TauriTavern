use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelProvidersRequestDto {
    pub model: String,
}
