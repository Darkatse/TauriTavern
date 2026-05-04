use async_trait::async_trait;

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentChatWorkspaceDeletion {
    pub workspace_id: String,
    pub removed: bool,
    pub run_ids: Vec<String>,
}

#[async_trait]
pub trait AgentWorkspaceLifecycleRepository: Send + Sync {
    async fn delete_chat_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<AgentChatWorkspaceDeletion, DomainError>;
}
