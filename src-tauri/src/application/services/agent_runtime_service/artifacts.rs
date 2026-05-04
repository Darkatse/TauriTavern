use chrono::Utc;

use super::AgentRuntimeService;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::{
    commit_policy_from_profile, workspace_roots_from_profile,
};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentRun, ArtifactTarget, WorkspaceInputManifest, WorkspaceManifest, WorkspacePath,
};
use crate::domain::repositories::workspace_repository::WorkspaceFile;

pub(super) fn build_agent_manifest(
    run: &AgentRun,
    profile: &ResolvedAgentProfile,
) -> WorkspaceManifest {
    WorkspaceManifest {
        workspace_version: 1,
        run_id: run.id.clone(),
        stable_chat_id: run.stable_chat_id.clone(),
        chat_ref: run.chat_ref.clone(),
        created_at: Utc::now(),
        input: WorkspaceInputManifest {
            mode: "prompt_snapshot".to_string(),
            prompt_snapshot_path: "input/prompt_snapshot.json".to_string(),
            resolved_profile_path: "input/resolved_profile.json".to_string(),
        },
        roots: workspace_roots_from_profile(profile),
        artifacts: profile.output.artifacts.clone(),
        commit_policy: commit_policy_from_profile(profile),
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
            return Err(ApplicationError::ValidationError(format!(
                "workspace.required_artifact_empty: {} is empty",
                final_path.as_str()
            )));
        }

        Ok(file)
    }
}
