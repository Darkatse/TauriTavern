use crate::application::services::agent_profile_service::{
    profile_model_configuration_error, profile_model_requires_configuration,
};
use crate::domain::models::agent::AgentRunPresentation;
use crate::domain::models::agent::profile::ResolvedAgentProfile;

pub(super) fn caller_allowed(source: &ResolvedAgentProfile, target: &ResolvedAgentProfile) -> bool {
    target
        .delegation
        .allowed_callers
        .iter()
        .any(|caller| caller == "*" || caller == source.id.as_str())
}

pub(super) fn validate_subagent_target(
    source: &ResolvedAgentProfile,
    target: &ResolvedAgentProfile,
) -> Result<(), String> {
    if !target.delegation.callable {
        return Err(format!(
            "agent.target_not_callable: profile `{}` is not callable by other Agents",
            target.id.as_str()
        ));
    }
    if !target.delegation.allow_as_subagent {
        return Err(format!(
            "agent.target_not_subagent: profile `{}` does not allow return-mode subagent calls",
            target.id.as_str()
        ));
    }
    if !caller_allowed(source, target) {
        return Err(format!(
            "agent.target_caller_denied: profile `{}` is not allowed to call `{}`",
            source.id.as_str(),
            target.id.as_str()
        ));
    }
    if profile_model_requires_configuration(target) {
        return Err(profile_model_configuration_error(target));
    }
    Ok(())
}

pub(super) fn validate_handoff_target(
    source: &ResolvedAgentProfile,
    target: &ResolvedAgentProfile,
) -> Result<(), String> {
    if !target.delegation.callable {
        return Err(format!(
            "agent.target_not_callable: Agent `{}` is not available for handoff",
            target.id.as_str()
        ));
    }
    if !target.delegation.allow_as_handoff_target {
        return Err(format!(
            "agent.target_not_handoff: Agent `{}` is not available as a handoff target",
            target.id.as_str()
        ));
    }
    if !caller_allowed(source, target) {
        return Err(format!(
            "agent.target_caller_denied: you are not allowed to hand off to Agent `{}`",
            target.id.as_str()
        ));
    }
    if profile_model_requires_configuration(target) {
        return Err(profile_model_configuration_error(target));
    }
    Ok(())
}

pub(super) fn apply_child_invocation_policy(profile: &mut ResolvedAgentProfile) {
    profile.run.presentation = AgentRunPresentation::Background;
    profile.tools.allow.retain(|name| {
        name != "workspace.commit"
            && name != "workspace.finish"
            && name != "agent.list"
            && name != "agent.delegate"
            && name != "agent.handoff"
            && name != "agent.await"
    });
}
