use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::{
    AgentModelBindingMode, AgentPresetBindingMode, ResolvedAgentProfile,
};
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentRunPresentation};

use super::constants::{CHAT_WORKSPACE_ROOTS, SESSION_WORKSPACE_ROOTS};
use super::require_output;

/// Writing requirements belong to a Chat execution, not the Profile file format.
pub fn validate_chat_profile(
    profile: &ResolvedAgentProfile,
    exit_policy: AgentInvocationExitPolicy,
    presentation: AgentRunPresentation,
) -> Result<(), ApplicationError> {
    validate_workspace_roots(profile, CHAT_WORKSPACE_ROOTS)?;
    require_output(profile)?;
    require_tool(
        profile,
        "workspace.write_file",
        "agent.profile_output_writer_required",
    )?;
    if exit_policy == AgentInvocationExitPolicy::RunFinishAllowed && profile.run.direct_runnable {
        require_tool(profile, "workspace.finish", "agent.profile_finish_required")?;
        if presentation == AgentRunPresentation::Foreground {
            require_tool(profile, "workspace.commit", "agent.profile_commit_required")?;
        }
    }
    Ok(())
}

pub(super) fn validate_session_profile(
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    if !profile.run.direct_runnable {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_not_direct_runnable: profile `{}` cannot start a root invocation",
            profile.id.as_str()
        )));
    }
    if profile.preset.mode != AgentPresetBindingMode::Ref {
        return Err(ApplicationError::ValidationError(
            "agent.session_preset_required: Sessions require an explicitly selected preset".into(),
        ));
    }
    if profile.model.mode != AgentModelBindingMode::ConnectionRef {
        return Err(ApplicationError::ValidationError(
            "agent.session_model_required: Sessions require an explicitly selected model connection".into(),
        ));
    }
    if profile.delegation.can_delegate || profile.delegation.can_handoff {
        return Err(ApplicationError::ValidationError(
            "agent.session_delegation_unsupported: Session delegation and handoff are not supported".into(),
        ));
    }
    validate_workspace_roots(profile, SESSION_WORKSPACE_ROOTS)?;
    Ok(())
}

fn validate_workspace_roots(
    profile: &ResolvedAgentProfile,
    roots: &[&str],
) -> Result<(), ApplicationError> {
    if let Some(root) = profile
        .workspace
        .visible_roots
        .iter()
        .find(|root| !roots.contains(&root.as_str()))
    {
        return Err(ApplicationError::ValidationError(format!(
            "agent.workspace_root_unavailable: root `{root}` is not available for this execution; supported roots: {}",
            roots.join(", ")
        )));
    }
    Ok(())
}

fn require_tool(
    profile: &ResolvedAgentProfile,
    name: &str,
    code: &str,
) -> Result<(), ApplicationError> {
    let visible =
        profile.tools.allow.iter().any(|id| {
            id.is_builtin() && id.native_name() == name && !profile.tools.deny.contains(id)
        });
    if !visible {
        return Err(ApplicationError::ValidationError(format!(
            "{code}: Chat execution requires {name}"
        )));
    }
    Ok(())
}
