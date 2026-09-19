use crate::workspace_fs::WorkspaceFs;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentRun, WorkspaceManifest, WorkspacePersistentChangeSet};

#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    /// Check the inherited version before creating a run or its workspace.
    async fn validate_persistent_state(
        &self,
        workspace_id: &str,
        state_id: &str,
    ) -> Result<(), DomainError>;

    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError>;

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError>;

    async fn open_filesystem(&self, run_id: &str) -> Result<Arc<dyn WorkspaceFs>, DomainError>;

    async fn commit_persistent_changes(
        &self,
        run_id: &str,
        previous_state_id: Option<&str>,
    ) -> Result<WorkspacePersistentChangeSet, DomainError>;
}
