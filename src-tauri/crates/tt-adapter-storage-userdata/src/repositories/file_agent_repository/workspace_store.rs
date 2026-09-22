use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;
use tt_ports::workspace_fs::WorkspaceFs;

use super::FileAgentRepository;
use super::paths::validate_workspace_root_path;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentRun, WorkspaceManifest, WorkspacePersistentChangeSet};
use tt_ports::repositories::workspace_repository::WorkspaceRepository;

#[async_trait]
impl WorkspaceRepository for FileAgentRepository {
    async fn validate_persistent_state(
        &self,
        workspace_id: &str,
        state_id: &str,
    ) -> Result<(), DomainError> {
        let state_dir = self.persistent_state_dir(workspace_id, state_id)?;
        self.read_persistent_state_manifest(&state_dir, state_id)
            .await?;
        Ok(())
    }

    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        let files = self.open_run_files(&run.id).await?;
        fs::create_dir_all(run_dir.join("input"))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create agent run input directory {}: {}",
                    run_dir.join("input").display(),
                    error
                ))
            })?;

        for root in &manifest.roots {
            let root_path = validate_workspace_root_path(&root.path)?;
            let target = files
                .resolve(&tt_domain::models::agent::WorkspacePath::parse(&root_path)?)
                .await?;
            fs::create_dir_all(&target).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create agent workspace root {}: {}",
                    target.display(),
                    error
                ))
            })?;
        }

        let persistent_snapshot = if run.target.session_id().is_none() {
            Some(
                self.initialize_projected_roots(run, manifest, &run_dir)
                    .await?,
            )
        } else {
            None
        };
        Self::write_json_atomic(&run_dir.join("manifest.json"), manifest).await?;
        Self::write_json_atomic(
            &run_dir.join("input").join("prompt_snapshot.json"),
            prompt_snapshot,
        )
        .await?;
        Self::write_json_atomic(
            &run_dir.join("input").join("resolved_profile.json"),
            resolved_profile,
        )
        .await?;
        if let Some(persistent_snapshot) = persistent_snapshot {
            Self::write_json_atomic(
                &run_dir.join("input").join("persist_snapshot.json"),
                &persistent_snapshot,
            )
            .await?;
        }
        Ok(())
    }

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        Self::read_json(&self.load_run_dir(run_id).await?.join("manifest.json")).await
    }

    async fn open_filesystem(
        &self,
        run_id: &str,
    ) -> Result<std::sync::Arc<dyn WorkspaceFs>, DomainError> {
        Ok(std::sync::Arc::new(self.open_run_files(run_id).await?))
    }

    async fn commit_persistent_changes(
        &self,
        run_id: &str,
        previous_state_id: Option<&str>,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let _guard = self.persist_lock.lock().await;
        let lock = self.workspace_lock(run_id).await;
        let _files = lock.read().await;
        self.publish_persistent_state(run_id, previous_state_id)
            .await
    }
}
