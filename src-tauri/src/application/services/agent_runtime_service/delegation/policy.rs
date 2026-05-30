use serde::Deserialize;

use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::{
    profile_model_configuration_error, profile_model_requires_configuration,
};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentRunPresentation, AgentTaskBudget};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AgentDelegateBudget {
    #[serde(default)]
    pub(super) max_rounds: Option<usize>,
    #[serde(default)]
    pub(super) max_tool_calls: Option<usize>,
}

impl From<AgentDelegateBudget> for AgentTaskBudget {
    fn from(value: AgentDelegateBudget) -> Self {
        Self {
            max_rounds: value.max_rounds,
            max_tool_calls: value.max_tool_calls,
        }
    }
}

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

pub(super) fn validate_delegate_budget(
    budget: Option<AgentDelegateBudget>,
    target: &ResolvedAgentProfile,
) -> Result<(), String> {
    let Some(budget) = budget else {
        return Ok(());
    };
    if let Some(max_rounds) = budget.max_rounds {
        if max_rounds == 0 {
            return Err("budget.maxRounds must be >= 1".to_string());
        }
        if max_rounds > target.tools.max_rounds {
            return Err(format!(
                "budget.maxRounds cannot exceed target profile maxRounds ({})",
                target.tools.max_rounds
            ));
        }
    }
    if let Some(max_tool_calls) = budget.max_tool_calls {
        if max_tool_calls == 0 {
            return Err("budget.maxToolCalls must be >= 1".to_string());
        }
        if max_tool_calls > target.tools.max_calls_per_run {
            return Err(format!(
                "budget.maxToolCalls cannot exceed target profile maxCallsPerRun ({})",
                target.tools.max_calls_per_run
            ));
        }
    }
    Ok(())
}

pub(super) fn apply_child_invocation_policy(
    profile: &mut ResolvedAgentProfile,
    budget: Option<AgentTaskBudget>,
) -> Result<(), ApplicationError> {
    profile.run.presentation = AgentRunPresentation::Background;
    profile.tools.allow.retain(|name| {
        name != "workspace.commit"
            && name != "workspace.finish"
            && name != "agent.list"
            && name != "agent.delegate"
            && name != "agent.await"
    });
    if let Some(budget) = budget {
        if let Some(max_rounds) = budget.max_rounds {
            profile.tools.max_rounds = max_rounds;
        }
        if let Some(max_tool_calls) = budget.max_tool_calls {
            profile.tools.max_calls_per_run = max_tool_calls;
        }
    }
    Ok(())
}
