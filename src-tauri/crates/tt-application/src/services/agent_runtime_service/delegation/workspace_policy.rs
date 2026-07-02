use async_trait::async_trait;

use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentRun, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
};
use tt_ports::repositories::workspace_repository::{
    WorkspaceAppendResult, WorkspaceFile, WorkspaceFileList, WorkspaceRepository,
    WorkspaceWriteGuard,
};

pub(in crate::services::agent_runtime_service) struct InvocationWorkspaceRepository<'a> {
    inner: &'a dyn WorkspaceRepository,
    profile: &'a ResolvedAgentProfile,
}

impl<'a> InvocationWorkspaceRepository<'a> {
    pub(in crate::services::agent_runtime_service) fn new(
        inner: &'a dyn WorkspaceRepository,
        profile: &'a ResolvedAgentProfile,
    ) -> Self {
        Self { inner, profile }
    }
}

#[async_trait]
impl WorkspaceRepository for InvocationWorkspaceRepository<'_> {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &serde_json::Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError> {
        self.inner
            .initialize_run(run, manifest, prompt_snapshot, resolved_profile)
            .await
    }

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        let mut manifest = self.inner.read_manifest(run_id).await?;
        for root in &mut manifest.roots {
            root.visible = self
                .profile
                .workspace
                .visible_roots
                .iter()
                .any(|visible| visible == &root.path);
            root.writable = self
                .profile
                .workspace
                .writable_roots
                .iter()
                .any(|writable| writable == &root.path);
        }
        Ok(manifest)
    }

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner.write_text(run_id, path, text).await
    }

    async fn write_text_guarded(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner
            .write_text_guarded(run_id, path, text, guard)
            .await
    }

    async fn append_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        self.inner.append_text(run_id, path, text).await
    }

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        self.inner.read_text(run_id, path).await
    }

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        self.inner
            .list_files(run_id, path, depth, max_entries)
            .await
    }

    async fn commit_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        self.inner.commit_persistent_changes(run_id).await
    }
}
