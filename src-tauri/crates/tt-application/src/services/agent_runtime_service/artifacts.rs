use chrono::Utc;

use crate::services::agent_profile_service::{
    commit_policy_from_profile, workspace_roots_from_profile,
};
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentRun, WorkspaceInputManifest, WorkspaceManifest};

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
