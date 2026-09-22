use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AgentModelMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionMessage {
    pub seq: u64,
    pub run_id: String,
    pub created_at: DateTime<Utc>,
    pub message: AgentModelMessage,
    /// Invocation output identity; user messages have no model origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<AgentSessionMessageOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionMessageOrigin {
    pub invocation_id: String,
    pub round: usize,
}
