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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub path: WorkspacePath,
    pub kind: WorkspaceEntryKind,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceFileList {
    pub entries: Vec<WorkspaceEntry>,
    pub truncated: bool,
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

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError>;
}
