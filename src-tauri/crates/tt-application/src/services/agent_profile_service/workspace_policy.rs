use std::collections::BTreeSet;

use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    ArtifactTarget, CommitPolicy, WorkspaceRootCommit, WorkspaceRootLifecycle, WorkspaceRootMount,
    WorkspaceRootScope, WorkspaceRootSpec,
};

use super::constants::WORKSPACE_ROOT_UNIVERSE;
use crate::services::agent_workspace_scope::AGENT_TOOL_RESULTS_ROOT;

pub fn workspace_roots_from_profile(profile: &ResolvedAgentProfile) -> Vec<WorkspaceRootSpec> {
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

    let mut roots = WORKSPACE_ROOT_UNIVERSE
        .iter()
        .map(|root| {
            if *root == "persist" {
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
        .collect::<Vec<_>>();
    roots.push(WorkspaceRootSpec {
        path: AGENT_TOOL_RESULTS_ROOT.to_string(),
        lifecycle: WorkspaceRootLifecycle::Run,
        scope: WorkspaceRootScope::Run,
        mount: WorkspaceRootMount::Materialized,
        visible: true,
        writable: false,
        commit: WorkspaceRootCommit::Never,
    });
    roots
}

pub fn commit_policy_from_profile(_profile: &ResolvedAgentProfile) -> CommitPolicy {
    CommitPolicy {
        default_target: ArtifactTarget::MessageBody,
        combine_template: None,
        store_artifacts_in_extra: true,
    }
}
