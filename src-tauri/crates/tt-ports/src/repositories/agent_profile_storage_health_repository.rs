use async_trait::async_trait;
pub use tt_contracts::agent_profile_storage::{
    AgentProfileStorageIssue, AgentProfileStorageIssueKind, AgentProfileStorageRepairAction,
    AgentProfileStorageScan,
};

use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::AgentProfileId;

#[async_trait]
pub trait AgentProfileStorageHealthRepository: Send + Sync {
    async fn scan_profiles(&self) -> Result<AgentProfileStorageScan, DomainError>;

    async fn normalize_profile_file_identity(&self, id: &AgentProfileId)
    -> Result<(), DomainError>;
}
