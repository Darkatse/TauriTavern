use std::collections::HashSet;

use super::workspace::{WORKSPACE_COMMIT, WORKSPACE_FINISH};
use super::{AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, BuiltinAgentToolRegistry, TASK_RETURN};
use crate::errors::ApplicationError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentModelTool};
use tt_domain::models::tool::{
    AgentToolScope, InvocationToolSnapshot, ToolBinding, ToolDescriptor, ToolId, ToolSnapshotId,
    ToolTurnContract,
};

const RETURN_MODE_DENIED_TOOLS: [&str; 5] = [
    WORKSPACE_COMMIT,
    WORKSPACE_FINISH,
    AGENT_DELEGATE,
    AGENT_HANDOFF,
    AGENT_AWAIT,
];

const SESSION_DENIED_TOOLS: [&str; 9] = [
    "chat.search",
    "chat.read_messages",
    "worldinfo.read_activated",
    WORKSPACE_COMMIT,
    WORKSPACE_FINISH,
    AGENT_DELEGATE,
    AGENT_HANDOFF,
    AGENT_AWAIT,
    TASK_RETURN,
];

/// Provider-specific discovery supplies metadata; selection and aliases are shared.
pub(crate) struct ExternalAgentTool {
    pub descriptor: ToolDescriptor,
    pub model_name: String,
}

pub(crate) fn builtin_available_in_scope(name: &str, scope: AgentToolScope) -> bool {
    scope != AgentToolScope::Session || !SESSION_DENIED_TOOLS.contains(&name)
}

pub(crate) fn prepare_tool_bindings(
    registry: &BuiltinAgentToolRegistry,
    profile: &ResolvedAgentProfile,
    scope: AgentToolScope,
    external_tools: &[ExternalAgentTool],
) -> Result<Vec<ToolBinding>, ApplicationError> {
    let mut bindings = Vec::with_capacity(profile.tools.allow.len());
    let mut aliases = profile
        .tools
        .allow
        .iter()
        .filter(|id| {
            id.is_builtin()
                && !profile.tools.deny.contains(id)
                && builtin_available_in_scope(id.native_name(), scope)
        })
        .map(|id| builtin_model_alias(id.native_name()))
        .collect::<HashSet<_>>();
    // Completion tools are added after discovery; keep their builtin names available.
    aliases.insert(builtin_model_alias(TASK_RETURN));
    for tool_id in &profile.tools.allow {
        if profile.tools.deny.contains(tool_id) {
            continue;
        }
        let binding = if tool_id.is_builtin() {
            if !builtin_available_in_scope(tool_id.native_name(), scope) {
                continue;
            }
            ToolBinding::new(
                registry.materialize_profile_descriptor(tool_id, profile)?,
                builtin_model_alias(tool_id.native_name()),
                profile.tools.max_calls_per_tool.get(tool_id).copied(),
            )?
        } else {
            let Some(tool) = external_tools
                .iter()
                .find(|tool| tool.descriptor.id == *tool_id)
            else {
                continue;
            };
            let alias = allocate_model_alias(&tool.model_name, &aliases);
            ToolBinding::new(
                tool.descriptor.clone(),
                alias,
                profile.tools.max_calls_per_tool.get(tool_id).copied(),
            )?
        };
        aliases.insert(binding.model_alias().to_string());
        bindings.push(binding);
    }
    Ok(bindings)
}

/// Completion tools belong to the invocation protocol, not provider discovery.
pub(crate) fn compile_invocation_tool_snapshot(
    registry: &BuiltinAgentToolRegistry,
    profile: &ResolvedAgentProfile,
    exit_policy: AgentInvocationExitPolicy,
    snapshot_id: ToolSnapshotId,
    mut bindings: Vec<ToolBinding>,
) -> Result<InvocationToolSnapshot, ApplicationError> {
    if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
        bindings.retain(|binding| {
            !binding.tool_id().is_builtin()
                || (!RETURN_MODE_DENIED_TOOLS.contains(&binding.tool_id().native_name())
                    && binding.tool_id().native_name() != TASK_RETURN)
        });
        for binding in &mut bindings {
            if binding.tool_id().is_builtin() {
                let mut descriptor = binding.descriptor().clone();
                registry.apply_return_mode_context(&mut descriptor, profile)?;
                *binding =
                    ToolBinding::new(descriptor, binding.model_alias(), binding.max_calls())?;
            }
        }
        let id = ToolId::builtin(TASK_RETURN)?;
        let mut descriptor = registry.materialize_profile_descriptor(&id, profile)?;
        registry.apply_return_mode_context(&mut descriptor, profile)?;
        bindings.push(ToolBinding::new(
            descriptor,
            builtin_model_alias(TASK_RETURN),
            profile.tools.max_calls_per_tool.get(&id).copied(),
        )?);
    }
    InvocationToolSnapshot::try_new(snapshot_id, bindings, profile.tools.max_calls_per_run)
        .map_err(Into::into)
}

pub(super) fn builtin_model_alias(name: &str) -> String {
    name.replace('.', "_")
}

const MAX_MODEL_ALIAS_BYTES: usize = 64;

fn allocate_model_alias(name: &str, used: &HashSet<String>) -> String {
    let name = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    for ordinal in 1_usize.. {
        let suffix = if ordinal == 1 {
            String::new()
        } else {
            format!("__{ordinal}")
        };
        let length = name.len().min(MAX_MODEL_ALIAS_BYTES - suffix.len());
        let alias = format!("{}{suffix}", &name[..length]);
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

pub(crate) fn mcp_model_name(server_name: &str, tool_name: &str) -> String {
    let server = normalize_alias_segment(server_name, "server");
    let tool = normalize_alias_segment(tool_name, "tool");
    const PREFIX: &str = "mcp__";
    const SEPARATOR: &str = "__";
    let available = MAX_MODEL_ALIAS_BYTES - PREFIX.len() - SEPARATOR.len();
    let (server_len, tool_len) = if server.len() + tool.len() <= available {
        (server.len(), tool.len())
    } else {
        let server_len = server.len().min(20).min(available / 2);
        let tool_len = tool.len().min(available - server_len);
        let server_len = server.len().min(available - tool_len);
        (server_len, tool_len)
    };
    format!(
        "{PREFIX}{}{SEPARATOR}{}",
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

    use super::{MAX_MODEL_ALIAS_BYTES, allocate_model_alias, mcp_model_name};

    #[test]
    fn model_aliases_remain_valid_and_unique_after_normalization_and_truncation() {
        let mcp_name = mcp_model_name(&"server".repeat(30), &"tool".repeat(40));
        let mut used = HashSet::new();
        for name in [
            "read.state".to_string(),
            "read_state".to_string(),
            "界".repeat(80),
            "界".repeat(81),
            mcp_name.clone(),
            mcp_name,
        ] {
            let alias = allocate_model_alias(&name, &used);
            assert!(!alias.is_empty() && alias.len() <= MAX_MODEL_ALIAS_BYTES);
            assert!(
                alias
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            );
            assert!(used.insert(alias));
        }
    }
}
