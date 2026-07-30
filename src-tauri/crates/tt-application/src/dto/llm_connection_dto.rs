use serde::{Deserialize, Serialize};

use tt_domain::models::llm_connection::{LlmConnectionDefinition, LlmConnectionSummary};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConnectionIdDto {
    pub connection_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLlmConnectionDto {
    pub connection: LlmConnectionDefinition,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLlmConnectionsResultDto {
    pub connections: Vec<LlmConnectionSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadLlmConnectionResultDto {
    pub connection: Option<LlmConnectionDefinition>,
}
