use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorRouteRequestDto {
    #[serde(default)]
    pub collection_id: String,
    pub collection_ids: Option<Vec<String>>,
    #[serde(default)]
    pub source: String,
    pub items: Option<Vec<VectorItemDto>>,
    pub hashes: Option<Vec<i64>>,
    #[serde(default)]
    pub search_text: String,
    pub top_k: Option<i64>,
    pub threshold: Option<f32>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_url: String,
    #[serde(default)]
    pub keep: bool,
    #[serde(default)]
    pub is_query: bool,
    #[serde(default)]
    pub extras_url: String,
    #[serde(default)]
    pub extras_key: String,
    #[serde(default)]
    pub embeddings: HashMap<String, Vec<f32>>,
    pub texts: Option<Vec<String>>,
    #[serde(default)]
    pub siliconflow_endpoint: String,
    #[serde(default)]
    pub workers_ai_account_id: String,
    #[serde(default)]
    pub vertexai_auth_mode: String,
    #[serde(default)]
    pub vertexai_region: String,
    #[serde(default)]
    pub vertexai_express_project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VectorItemDto {
    pub hash: i64,
    pub text: String,
    #[serde(default)]
    pub index: i64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorRouteResponseKindDto {
    Json,
    Empty,
}

#[derive(Debug, Clone, Serialize)]
pub struct VectorRouteResponseDto {
    pub status: u16,
    pub kind: VectorRouteResponseKindDto,
    pub body: Value,
}
