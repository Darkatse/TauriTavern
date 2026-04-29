use chrono::Utc;

use super::AgentRuntimeService;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AgentRun, ArtifactSpec, ArtifactTarget, CommitPolicy, WorkspaceInputManifest,
    WorkspaceManifest, WorkspacePath, WorkspaceRootCommit, WorkspaceRootLifecycle,
    WorkspaceRootMount, WorkspaceRootScope, WorkspaceRootSpec,
};
use crate::domain::repositories::workspace_repository::WorkspaceFile;

pub(super) fn build_agent_manifest(run: &AgentRun) -> WorkspaceManifest {
    WorkspaceManifest {
        workspace_version: 1,
        run_id: run.id.clone(),
        stable_chat_id: run.stable_chat_id.clone(),
        chat_ref: run.chat_ref.clone(),
        created_at: Utc::now(),
        input: WorkspaceInputManifest {
            mode: "prompt_snapshot".to_string(),
            prompt_snapshot_path: "input/prompt_snapshot.json".to_string(),
        },
        roots: default_workspace_roots(),
        artifacts: vec![ArtifactSpec {
            id: "main".to_string(),
            path: "output/main.md".to_string(),
            kind: "markdown".to_string(),
            target: ArtifactTarget::MessageBody,
            required: true,
            assembly_order: 0,
        }],
        commit_policy: CommitPolicy {
            default_target: ArtifactTarget::MessageBody,
            combine_template: None,
            store_artifacts_in_extra: true,
        },
    }
}

fn default_workspace_roots() -> Vec<WorkspaceRootSpec> {
    vec![
        run_root("output"),
        run_root("scratch"),
        run_root("plan"),
        run_root("summaries"),
        WorkspaceRootSpec {
            path: "persist".to_string(),
            lifecycle: WorkspaceRootLifecycle::Persistent,
            scope: WorkspaceRootScope::Chat,
            mount: WorkspaceRootMount::ProjectedOverlay,
            visible: true,
            writable: true,
            commit: WorkspaceRootCommit::OnRunCompleted,
        },
    ]
}

fn run_root(path: &str) -> WorkspaceRootSpec {
    WorkspaceRootSpec {
        path: path.to_string(),
        lifecycle: WorkspaceRootLifecycle::Run,
        scope: WorkspaceRootScope::Run,
        mount: WorkspaceRootMount::Materialized,
        visible: true,
        writable: true,
        commit: WorkspaceRootCommit::Never,
    }
}

impl AgentRuntimeService {
    pub(super) async fn validate_final_artifact(
        &self,
        run_id: &str,
        manifest: &WorkspaceManifest,
        final_path: &WorkspacePath,
    ) -> Result<WorkspaceFile, ApplicationError> {
        let message_artifact = manifest
            .artifacts
            .iter()
            .find(|artifact| matches!(artifact.target, ArtifactTarget::MessageBody))
            .ok_or_else(|| {
                ApplicationError::ValidationError(
                    "workspace.message_body_artifact_missing: manifest does not declare a message body artifact"
                        .to_string(),
                )
            })?;

        if final_path.as_str() != message_artifact.path {
            return Err(ApplicationError::ValidationError(format!(
                "workspace.final_path_mismatch: expected {}, got {}",
                message_artifact.path,
                final_path.as_str()
            )));
        }

        let file = self
            .workspace_repository
            .read_text(run_id, final_path)
            .await?;
        if message_artifact.required && file.text.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "workspace.required_artifact_empty: output/main.md is empty".to_string(),
            ));
        }

        Ok(file)
    }
}
