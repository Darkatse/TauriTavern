use std::collections::BTreeSet;

use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::{
    AgentOutputArtifactTarget, AgentOutputPolicy, AgentWorkspacePolicy, ResolvedAgentOutputPolicy,
};
use tt_domain::models::agent::{ArtifactSpec, WorkspacePath};

use super::constants::MESSAGE_BODY_ARTIFACT_TARGET;

pub(super) fn resolve_output_policy(
    policy: &AgentOutputPolicy,
    workspace: &AgentWorkspacePolicy,
) -> Result<ResolvedAgentOutputPolicy, ApplicationError> {
    if policy.artifacts.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_output_empty: output.artifacts cannot be empty".to_string(),
        ));
    }

    let visible = workspace
        .visible_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();
    let writable = workspace
        .writable_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();

    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut message_body_artifact = None;
    let mut artifacts = Vec::with_capacity(policy.artifacts.len());
    for artifact in &policy.artifacts {
        validate_artifact_id(&artifact.id)?;
        if !ids.insert(artifact.id.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_duplicate_id: duplicate artifact id `{}`",
                artifact.id
            )));
        }
        if artifact.kind.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_kind_required: artifact `{}` kind cannot be empty",
                artifact.id
            )));
        }
        let path = WorkspacePath::parse(&artifact.path)?;
        if !paths.insert(path.as_str().to_string()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_duplicate_path: duplicate artifact path `{}`",
                path.as_str()
            )));
        }
        let root = path.as_str().split('/').next().unwrap_or_default();
        if !visible.contains(root) || !writable.contains(root) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_output_path_denied: artifact `{}` path `{}` must be visible and writable",
                artifact.id,
                path.as_str()
            )));
        }

        let target = match artifact.target {
            AgentOutputArtifactTarget::MessageBody => {
                if message_body_artifact.is_some() {
                    return Err(ApplicationError::ValidationError(
                        "agent.profile_output_duplicate_message_body: only one messageBody artifact is supported"
                            .to_string(),
                    ));
                }
                message_body_artifact = Some((artifact.id.clone(), path.as_str().to_string()));
                MESSAGE_BODY_ARTIFACT_TARGET
            }
        };

        artifacts.push(ArtifactSpec {
            id: artifact.id.clone(),
            path: path.as_str().to_string(),
            kind: artifact.kind.trim().to_string(),
            target,
            required: artifact.required,
            assembly_order: artifact.assembly_order,
        });
    }

    let Some((message_body_artifact_id, message_body_path)) = message_body_artifact else {
        return Err(ApplicationError::ValidationError(
            "agent.profile_output_message_body_missing: output.artifacts must include one messageBody artifact"
                .to_string(),
        ));
    };

    Ok(ResolvedAgentOutputPolicy {
        artifacts,
        message_body_artifact_id,
        message_body_path,
    })
}

fn validate_artifact_id(id: &str) -> Result<(), ApplicationError> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_artifact_id_invalid: invalid artifact id `{id}`"
        )));
    }
    Ok(())
}
