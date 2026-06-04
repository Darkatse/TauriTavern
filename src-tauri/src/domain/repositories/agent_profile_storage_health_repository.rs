use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::{AgentProfileId, AgentProfileSummary};

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

#[async_trait]
pub trait AgentProfileStorageHealthRepository: Send + Sync {
    async fn scan_profiles(&self) -> Result<AgentProfileStorageScan, DomainError>;

    async fn normalize_profile_file_identity(&self, id: &AgentProfileId)
    -> Result<(), DomainError>;
}
