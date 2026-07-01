use serde::{Deserialize, Serialize};
use tt_domain::models::agent::profile::{AgentProfileId, AgentProfileSummary};

#[derive(Debug, Clone, Default)]
pub struct AgentProfileStorageScan {
    pub profiles: Vec<AgentProfileSummary>,
    pub issues: Vec<AgentProfileStorageIssue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileStorageIssue {
    pub profile_id: AgentProfileId,
    pub file_name: String,
    pub kind: AgentProfileStorageIssueKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<AgentProfileStorageRepairAction>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProfileStorageIssueKind {
    InvalidJson,
    InvalidFileIdentity,
    InvalidProfile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProfileStorageRepairAction {
    Delete,
    NormalizeIdentity,
}
