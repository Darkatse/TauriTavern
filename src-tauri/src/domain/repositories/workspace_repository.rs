use async_trait::async_trait;
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentRun, WorkspaceManifest, WorkspacePath};

#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub path: WorkspacePath,
    pub text: String,
    pub bytes: u64,
    pub sha256: String,
}

#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
    ) -> Result<(), DomainError>;

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError>;

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError>;

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError>;
}
