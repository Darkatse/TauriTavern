use super::workspace::{WORKSPACE_COMMIT, WORKSPACE_FINISH};
use super::{
    AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, AGENT_LIST, BuiltinAgentToolRegistry, TASK_RETURN,
};
use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentToolSpec};
use tt_domain::models::tool::{
    InvocationToolSnapshot, ToolBinding, ToolId, ToolSnapshotId, ToolTurnContract,
};

const RETURN_MODE_DENIED_TOOLS: [&str; 6] = [
    WORKSPACE_COMMIT,
    WORKSPACE_FINISH,
    AGENT_LIST,
    AGENT_DELEGATE,
    AGENT_HANDOFF,
    AGENT_AWAIT,
];

pub(crate) fn compile_invocation_tool_snapshot(
    registry: &BuiltinAgentToolRegistry,
    profile: &ResolvedAgentProfile,
    exit_policy: AgentInvocationExitPolicy,
    snapshot_id: ToolSnapshotId,
) -> Result<InvocationToolSnapshot, ApplicationError> {
    let mut bindings = Vec::with_capacity(profile.tools.allow.len() + 1);
    for name in &profile.tools.allow {
        if profile.tools.deny.iter().any(|denied| denied == name) {
            continue;
        }
        if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired
            && RETURN_MODE_DENIED_TOOLS.contains(&name.as_str())
        {
            continue;
        }
        bindings.push(materialize_binding(registry, profile, name, exit_policy)?);
    }

    if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
        bindings.push(materialize_binding(
            registry,
            profile,
            TASK_RETURN,
            exit_policy,
        )?);
    }

    InvocationToolSnapshot::try_new(snapshot_id, bindings, profile.tools.max_calls_per_run)
        .map_err(Into::into)
}

fn materialize_binding(
    registry: &BuiltinAgentToolRegistry,
    profile: &ResolvedAgentProfile,
    name: &str,
    exit_policy: AgentInvocationExitPolicy,
) -> Result<ToolBinding, ApplicationError> {
    let tool_id = ToolId::builtin(name)?;
    let mut descriptor = registry.materialize_profile_descriptor(&tool_id, profile)?;
    if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
        registry.apply_return_mode_context(&mut descriptor, profile)?;
    }
    let alias = registry.model_alias(&tool_id)?.to_string();
    let max_calls = profile.tools.max_calls_per_tool.get(name).copied();
    ToolBinding::new(descriptor, alias, max_calls).map_err(Into::into)
}

pub(crate) fn project_agent_tool_specs(
    snapshot: &InvocationToolSnapshot,
    turn: &ToolTurnContract,
) -> Result<Vec<AgentToolSpec>, ApplicationError> {
    if turn.snapshot_id() != snapshot.id() {
        return Err(ApplicationError::ValidationError(format!(
            "tool.turn_snapshot_mismatch: turn references snapshot `{}` but `{}` was supplied",
            turn.snapshot_id(),
            snapshot.id()
        )));
    }

    turn.tools()
        .iter()
        .map(|tool_id| {
            let binding = snapshot.binding(tool_id).ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "tool.turn_tool_not_in_snapshot: tool `{tool_id}` is not in snapshot `{}`",
                    snapshot.id()
                ))
            })?;
            let descriptor = binding.descriptor();
            let title = descriptor.title.clone().ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.tool_title_required: builtin tool `{tool_id}` has no title"
                ))
            })?;
            let description = descriptor.description.clone().ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.tool_description_required: builtin tool `{tool_id}` has no description"
                ))
            })?;

            Ok(AgentToolSpec {
                name: tool_id.native_name().to_string(),
                model_name: binding.model_alias().to_string(),
                title,
                description,
                input_schema: descriptor.input_schema.clone(),
                output_schema: descriptor.output_schema.clone(),
                annotations: descriptor.annotations.clone(),
                source: tool_id.provider_id().to_string(),
            })
        })
        .collect()
}
