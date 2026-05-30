use async_trait::async_trait;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentInvocation, AgentTaskRecord};

#[async_trait]
pub trait AgentInvocationRepository: Send + Sync {
    async fn save_invocation(&self, invocation: &AgentInvocation) -> Result<(), DomainError>;

    /// Load an invocation that must already exist.
    async fn load_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<AgentInvocation, DomainError>;

    /// Load an invocation when absence is an expected control-flow branch.
    async fn try_load_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<Option<AgentInvocation>, DomainError>;

    async fn list_invocations(&self, run_id: &str) -> Result<Vec<AgentInvocation>, DomainError>;

    async fn save_task(&self, task: &AgentTaskRecord) -> Result<(), DomainError>;

    async fn load_task(&self, run_id: &str, task_id: &str) -> Result<AgentTaskRecord, DomainError>;

    async fn list_tasks(&self, run_id: &str) -> Result<Vec<AgentTaskRecord>, DomainError>;
}
