use crate::services::agent_profile_service::{
    profile_model_requires_configuration, validate_chat_profile,
};
use crate::services::agent_tools::{AGENT_DELEGATE, AGENT_HANDOFF};
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentRunPresentation};

#[derive(Clone, Copy)]
pub(super) enum AgentOperation {
    Delegate,
    Handoff,
}

impl AgentOperation {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Delegate => "delegate",
            Self::Handoff => "handoff",
        }
    }

    pub(super) fn tool_name(self) -> &'static str {
        match self {
            Self::Delegate => AGENT_DELEGATE,
            Self::Handoff => AGENT_HANDOFF,
        }
    }

    pub(super) fn validate_target(
        self,
        source: &ResolvedAgentProfile,
        target: &ResolvedAgentProfile,
    ) -> Result<(), String> {
        let id = target.id.as_str();
        if !target.delegation.callable
            || !target
                .delegation
                .allowed_callers
                .iter()
                .any(|caller| caller == "*" || caller == source.id.as_str())
        {
            return Err(format!("Agent `{id}` is not available to you."));
        }
        match self {
            Self::Delegate if !target.delegation.allow_as_subagent => {
                return Err(format!("Agent `{id}` does not accept delegated tasks."));
            }
            Self::Handoff if !target.delegation.allow_as_handoff_target => {
                return Err(format!("Agent `{id}` does not accept handoffs."));
            }
            _ => {}
        }
        if profile_model_requires_configuration(target) {
            return Err(format!("Agent `{id}` has no model configured."));
        }
        let (exit_policy, presentation) = match self {
            Self::Delegate => (
                AgentInvocationExitPolicy::TaskReturnRequired,
                AgentRunPresentation::Background,
            ),
            Self::Handoff => (
                AgentInvocationExitPolicy::RunFinishAllowed,
                target.run.presentation,
            ),
        };
        validate_chat_profile(target, exit_policy, presentation)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
