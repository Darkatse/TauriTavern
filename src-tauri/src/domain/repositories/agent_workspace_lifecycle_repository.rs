use async_trait::async_trait;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentChatWorkspaceDeletion {
    pub workspace_id: String,
    pub removed: bool,
    pub run_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPersistentStatePrune {
    pub workspace_id: String,
    pub removed_state_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPersistentStatePruneRequest {
    pub retained_state_ids: Vec<String>,
    pub candidate_state_ids: Vec<String>,
}

#[async_trait]
pub trait AgentWorkspaceLifecycleRepository: Send + Sync {
    async fn delete_chat_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<AgentChatWorkspaceDeletion, DomainError>;

    async fn prune_persistent_states(
        &self,
        workspace_id: &str,
        request: AgentPersistentStatePruneRequest,
    ) -> Result<AgentPersistentStatePrune, DomainError>;
}
