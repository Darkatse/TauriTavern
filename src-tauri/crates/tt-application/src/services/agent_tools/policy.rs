use std::collections::HashSet;

use super::workspace::{WORKSPACE_COMMIT, WORKSPACE_FINISH};
use super::{
    AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, AGENT_LIST, BuiltinAgentToolRegistry, TASK_RETURN,
};
use crate::errors::ApplicationError;
use crate::services::mcp_service::AgentMcpTool;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentModelTool};
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
    mcp_tools: &[AgentMcpTool],
) -> Result<InvocationToolSnapshot, ApplicationError> {
    let mut bindings = Vec::with_capacity(profile.tools.allow.len() + 1);
    let mut aliases = HashSet::with_capacity(profile.tools.allow.len() + 1);
    for tool_id in &profile.tools.allow {
        if profile.tools.deny.iter().any(|denied| denied == tool_id) {
            continue;
        }
        if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired
            && tool_id.is_builtin()
            && RETURN_MODE_DENIED_TOOLS.contains(&tool_id.native_name())
        {
            continue;
        }
        let binding = if tool_id.is_builtin() {
            materialize_builtin_binding(registry, profile, tool_id, exit_policy)?
        } else {
            let Some(tool) = mcp_tools.iter().find(|tool| tool.descriptor.id == *tool_id) else {
                continue;
            };
            let alias =
                allocate_mcp_alias(&tool.server_display_name, tool_id.native_name(), &aliases);
            ToolBinding::new(
                tool.descriptor.clone(),
                alias,
                profile.tools.max_calls_per_tool.get(tool_id).copied(),
            )?
        };
        aliases.insert(binding.model_alias().to_string());
        bindings.push(binding);
    }

    if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
        let binding = materialize_builtin_binding(
            registry,
            profile,
            &ToolId::builtin(TASK_RETURN)?,
            exit_policy,
        )?;
        aliases.insert(binding.model_alias().to_string());
        bindings.push(binding);
    }

    InvocationToolSnapshot::try_new(snapshot_id, bindings, profile.tools.max_calls_per_run)
        .map_err(Into::into)
}

fn materialize_builtin_binding(
    registry: &BuiltinAgentToolRegistry,
    profile: &ResolvedAgentProfile,
    tool_id: &ToolId,
    exit_policy: AgentInvocationExitPolicy,
) -> Result<ToolBinding, ApplicationError> {
    let mut descriptor = registry.materialize_profile_descriptor(tool_id, profile)?;
    if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
        registry.apply_return_mode_context(&mut descriptor, profile)?;
    }
    let alias = tool_id.native_name().replace('.', "_");
    let max_calls = profile.tools.max_calls_per_tool.get(tool_id).copied();
    ToolBinding::new(descriptor, alias, max_calls).map_err(Into::into)
}

const MAX_MODEL_ALIAS_BYTES: usize = 64;

fn allocate_mcp_alias(server_name: &str, tool_name: &str, used: &HashSet<String>) -> String {
    let server = normalize_alias_segment(server_name, "server");
    let tool = normalize_alias_segment(tool_name, "tool");
    for ordinal in 1_usize.. {
        let suffix = if ordinal == 1 {
            String::new()
        } else {
            format!("__{ordinal}")
        };
        let alias = fit_mcp_alias(&server, &tool, &suffix);
        if !used.contains(&alias) {
            return alias;
        }
    }
    unreachable!("an increasing numeric suffix always yields a unique alias")
}

fn normalize_alias_segment(value: &str, fallback: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_underscore = false;
    for character in value.chars() {
        let character = if character.is_ascii_alphanumeric() || character == '-' {
            character
        } else {
            '_'
        };
        if character == '_' {
            if last_was_underscore {
                continue;
            }
            last_was_underscore = true;
        } else {
            last_was_underscore = false;
        }
        normalized.push(character);
    }
    let normalized = normalized.trim_matches('_');
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized.to_string()
    }
}

fn fit_mcp_alias(server: &str, tool: &str, suffix: &str) -> String {
    const PREFIX: &str = "mcp__";
    const SEPARATOR: &str = "__";
    let available = MAX_MODEL_ALIAS_BYTES - PREFIX.len() - SEPARATOR.len() - suffix.len();
    let (server_len, tool_len) = if server.len() + tool.len() <= available {
        (server.len(), tool.len())
    } else {
        let server_len = server.len().min(20).min(available / 2);
        let tool_len = tool.len().min(available - server_len);
        let server_len = server.len().min(available - tool_len);
        (server_len, tool_len)
    };
    format!(
        "{PREFIX}{}{SEPARATOR}{}{suffix}",
        &server[..server_len],
        &tool[..tool_len]
    )
}

pub(crate) fn project_agent_model_tools(
    snapshot: &InvocationToolSnapshot,
    turn: &ToolTurnContract,
) -> Result<Vec<AgentModelTool>, ApplicationError> {
    if turn.snapshot_id() != snapshot.id() {
        return Err(ApplicationError::InternalError(format!(
            "tool.turn_snapshot_mismatch: turn references snapshot `{}` but `{}` was supplied",
            turn.snapshot_id(),
            snapshot.id()
        )));
    }

    turn.tools()
        .iter()
        .map(|tool_id| {
            let binding = snapshot.binding(tool_id).ok_or_else(|| {
                ApplicationError::InternalError(format!(
                    "tool.turn_tool_not_in_snapshot: tool `{tool_id}` is not in snapshot `{}`",
                    snapshot.id()
                ))
            })?;
            let descriptor = binding.descriptor();
            Ok(AgentModelTool {
                tool_id: tool_id.clone(),
                model_alias: binding.model_alias().to_string(),
                description: descriptor.description.clone(),
                input_schema: descriptor.input_schema.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{MAX_MODEL_ALIAS_BYTES, allocate_mcp_alias};

    #[test]
    fn mcp_aliases_are_readable_bounded_and_collision_safe() {
        let base = allocate_mcp_alias("my server", "issue.create", &HashSet::new());
        assert_eq!(base, "mcp__my_server__issue_create");

        let second =
            allocate_mcp_alias("my.server", "issue create", &HashSet::from([base.clone()]));
        assert_eq!(second, "mcp__my_server__issue_create__2");

        let long = allocate_mcp_alias(&"server".repeat(30), &"tool".repeat(40), &HashSet::new());
        assert!(long.len() <= MAX_MODEL_ALIAS_BYTES);
        assert!(long.starts_with("mcp__"));
    }
}
