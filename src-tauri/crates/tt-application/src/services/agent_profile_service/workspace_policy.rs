use std::collections::BTreeSet;

use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentRunTarget, ArtifactTarget, CommitPolicy, WorkspaceRootCommit, WorkspaceRootLifecycle,
    WorkspaceRootMount, WorkspaceRootScope, WorkspaceRootSpec,
};

use super::constants::{CHAT_WORKSPACE_ROOTS, SESSION_WORKSPACE_ROOTS};

pub fn workspace_roots_from_profile(
    profile: &ResolvedAgentProfile,
    target: &AgentRunTarget,
) -> Vec<WorkspaceRootSpec> {
    let visible = profile
        .workspace
        .visible_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();
    let writable = profile
        .workspace
        .writable_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();

    let session = target.session_id().is_some();
    let root_names: &[&str] = if session {
        SESSION_WORKSPACE_ROOTS
    } else {
        CHAT_WORKSPACE_ROOTS
    };
    root_names
        .iter()
        .map(|root| {
            if session {
                WorkspaceRootSpec {
                    path: root.to_string(),
                    lifecycle: WorkspaceRootLifecycle::Persistent,
                    scope: WorkspaceRootScope::Session,
                    mount: WorkspaceRootMount::Materialized,
                    visible: visible.contains(*root),
                    writable: writable.contains(*root),
                    commit: WorkspaceRootCommit::Never,
                }
            } else if *root == "persist" {
                WorkspaceRootSpec {
                    path: root.to_string(),
                    lifecycle: WorkspaceRootLifecycle::Persistent,
                    scope: WorkspaceRootScope::Chat,
                    mount: WorkspaceRootMount::ProjectedOverlay,
                    visible: visible.contains(*root),
                    writable: writable.contains(*root),
                    commit: WorkspaceRootCommit::OnRunCompleted,
                }
            } else {
                WorkspaceRootSpec {
                    path: root.to_string(),
                    lifecycle: WorkspaceRootLifecycle::Run,
                    scope: WorkspaceRootScope::Run,
                    mount: WorkspaceRootMount::Materialized,
                    visible: visible.contains(*root),
                    writable: writable.contains(*root),
                    commit: WorkspaceRootCommit::Never,
                }
            }
        })
        .collect()
}

pub fn commit_policy_from_profile(_profile: &ResolvedAgentProfile) -> CommitPolicy {
    CommitPolicy {
        default_target: ArtifactTarget::MessageBody,
        combine_template: None,
        store_artifacts_in_extra: true,
    }
}
