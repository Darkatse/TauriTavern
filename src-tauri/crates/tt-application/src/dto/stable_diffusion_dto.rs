use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SdRouteResponseKindDto {
    Json,
    Text,
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdRouteResponseDto {
    pub status: u16,
    pub kind: SdRouteResponseKindDto,
    #[serde(default)]
    pub body: Value,
}
