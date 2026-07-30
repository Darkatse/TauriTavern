use async_trait::async_trait;
use serde_json::Value;

use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentRun, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
};

#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub path: WorkspacePath,
    pub text: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceAppendResult {
    pub file: WorkspaceFile,
    pub previous_sha256: Option<String>,
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
}

#[derive(Debug, Clone)]
pub struct WorkspaceFileList {
    pub entries: Vec<WorkspaceEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceWriteGuard {
    Unchecked,
    MustNotExist,
    MustMatchSha256(String),
}

#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError>;

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError>;

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError>;

    async fn write_text_guarded(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError>;

    async fn append_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError>;

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

    async fn commit_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError>;
}
