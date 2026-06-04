use std::collections::BTreeSet;

use serde_json::Value;

use crate::application::errors::ApplicationError;
use crate::domain::models::agent::AgentRunPresentation;
use crate::domain::models::agent::AgentToolSpec;
use crate::domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
use crate::domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy, AgentDelegationPolicy,
    AgentModelBinding, AgentModelBindingMode, AgentProfileDefinition, AgentProfileId,
    AgentProfileInstructions, AgentRunPolicy, AgentSkillPolicy, AgentToolDescriptionOverride,
    AgentToolPolicy, AgentWorkspacePolicy,
};

use super::constants::{
    AGENT_AWAIT_TOOL, AGENT_DELEGATE_TOOL, AGENT_LIST_TOOL, TASK_RETURN_TOOL,
    WORKSPACE_ROOT_UNIVERSE,
};

pub(super) fn validate_profile_header(
    profile: &AgentProfileDefinition,
) -> Result<(), ApplicationError> {
    if profile.schema_version != AGENT_PROFILE_SCHEMA_VERSION {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_schema_unsupported: schemaVersion {} is unsupported",
            profile.schema_version
        )));
    }
    if profile.kind != AGENT_PROFILE_KIND {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_kind_invalid: kind must be {AGENT_PROFILE_KIND}"
        )));
    }
    if profile.display_name.trim().is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_display_name_required: displayName cannot be empty".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn migrate_profile_schema(
    profile: &mut AgentProfileDefinition,
) -> Result<(), ApplicationError> {
    match profile.schema_version {
        1 => {
            profile.schema_version = AGENT_PROFILE_SCHEMA_VERSION;
            Ok(())
        }
        AGENT_PROFILE_SCHEMA_VERSION => Ok(()),
        version => Err(ApplicationError::ValidationError(format!(
            "agent.profile_schema_unsupported: schemaVersion {version} is unsupported"
        ))),
    }
}

pub(super) fn validate_model_binding(binding: &AgentModelBinding) -> Result<(), ApplicationError> {
    match binding.mode {
        AgentModelBindingMode::CurrentPromptSnapshot => {
            if binding
                .connection_ref
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
                || binding
                    .model_id
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
            {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_model_current_snapshot_extra_fields: connectionRef/modelId are only valid when model.mode is connectionRef"
                        .to_string(),
                ));
            }
            Ok(())
        }
        AgentModelBindingMode::RequiresConfiguration => {
            if binding
                .connection_ref
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
                || binding
                    .model_id
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
            {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_model_requires_configuration_extra_fields: connectionRef/modelId must be empty when model.mode is requiresConfiguration"
                        .to_string(),
                ));
            }
            Ok(())
        }
        AgentModelBindingMode::ConnectionRef => {
            if binding
                .connection_ref
                .as_ref()
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_model_connection_ref_required: model.connectionRef is required when model.mode is connectionRef"
                        .to_string(),
                ));
            }
            if binding
                .model_id
                .as_ref()
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(ApplicationError::ValidationError(
                    "agent.profile_model_id_required: model.modelId is required when model.mode is connectionRef"
                        .to_string(),
                ));
            }
            Ok(())
        }
    }
}

pub(super) fn normalize_context_policy(
    policy: &mut AgentContextPolicy,
) -> Result<(), ApplicationError> {
    if policy.initial_chat_history_messages < 0 {
        policy.initial_chat_history_messages = -1;
    }
    Ok(())
}

pub(super) fn validate_instructions(
    instructions: &AgentProfileInstructions,
) -> Result<(), ApplicationError> {
    if instructions
        .agent_system_prompt
        .as_ref()
        .is_some_and(|prompt| prompt.trim().is_empty())
    {
        return Err(ApplicationError::ValidationError(
            "agent.profile_system_prompt_empty: instructions.agentSystemPrompt cannot be empty"
                .to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_plan_policy(plan: &AgentPlanPolicy) -> Result<(), ApplicationError> {
    if plan.mode != AgentPlanMode::None {
        return Err(ApplicationError::ValidationError(
            "agent.plan_mode_unsupported: Phase 3 foundation only supports plan.mode = none"
                .to_string(),
        ));
    }
    if !plan.nodes.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.plan_nodes_unsupported: plan.nodes must be empty when plan.mode = none"
                .to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_tool_policy(
    policy: &AgentToolPolicy,
    known_tools: &[AgentToolSpec],
) -> Result<(), ApplicationError> {
    if policy.max_rounds == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_max_rounds_invalid: tools.maxRounds must be > 0".to_string(),
        ));
    }
    if policy.max_calls_per_run == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_max_calls_invalid: tools.maxCallsPerRun must be > 0".to_string(),
        ));
    }

    let known = known_tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<BTreeSet<_>>();
    let allow = policy
        .allow
        .iter()
        .map(|name| name.as_str())
        .collect::<BTreeSet<_>>();
    let deny = policy
        .deny
        .iter()
        .map(|name| name.as_str())
        .collect::<BTreeSet<_>>();

    if allow.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_tools_empty: tools.allow cannot be empty".to_string(),
        ));
    }
    for name in allow.iter().chain(deny.iter()) {
        if !known.contains(name) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_unknown_tool: unknown tool `{name}`"
            )));
        }
    }
    let visible = allow.difference(&deny).copied().collect::<BTreeSet<_>>();
    if !visible.contains("workspace.write_file") {
        return Err(ApplicationError::ValidationError(
            "agent.profile_output_writer_required: workspace.write_file must be visible so the Agent can create the required message body artifact"
                .to_string(),
        ));
    }
    for (name, override_) in &policy.tool_descriptions {
        if !visible.contains(name.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_description_invisible: `{name}` is not visible"
            )));
        }
        let spec = known_tools
            .iter()
            .find(|tool| tool.name == *name)
            .expect("known tool already checked");
        validate_tool_description_override(spec, override_)?;
    }

    for (name, max) in &policy.max_calls_per_tool {
        if !visible.contains(name.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_budget_invisible: `{name}` is not visible"
            )));
        }
        if *max == 0 {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_budget_invalid: maxCallsPerTool.{name} must be > 0"
            )));
        }
    }

    Ok(())
}

pub(super) fn validate_delegation_policy(
    policy: &AgentDelegationPolicy,
    tools: &AgentToolPolicy,
) -> Result<(), ApplicationError> {
    if policy.max_concurrent_invocations == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_delegation_concurrency_invalid: delegation.maxConcurrentInvocations must be > 0"
                .to_string(),
        ));
    }
    if policy.max_invocations_per_run == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_delegation_run_budget_invalid: delegation.maxInvocationsPerRun must be > 0"
                .to_string(),
        ));
    }
    if policy.result_budget_tokens == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_delegation_result_budget_invalid: delegation.resultBudgetTokens must be > 0"
                .to_string(),
        ));
    }
    if policy.max_handoff_depth == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_handoff_depth_invalid: delegation.maxHandoffDepth must be > 0"
                .to_string(),
        ));
    }
    if policy.allowed_callers.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_delegation_callers_empty: delegation.allowedCallers cannot be empty"
                .to_string(),
        ));
    }
    for caller in &policy.allowed_callers {
        if caller == "*" {
            continue;
        }
        AgentProfileId::parse(caller).map_err(ApplicationError::ValidationError)?;
    }
    if policy
        .description_for_agents
        .as_ref()
        .is_some_and(|description| description.trim().is_empty())
    {
        return Err(ApplicationError::ValidationError(
            "agent.profile_delegation_description_empty: delegation.descriptionForAgents cannot be empty"
                .to_string(),
        ));
    }

    if !policy.callable && (policy.allow_as_subagent || policy.allow_as_handoff_target) {
        return Err(ApplicationError::ValidationError(
            "agent.profile_delegation_callable_required: delegation.callable must be true before allowing this profile as a subagent or handoff target"
                .to_string(),
        ));
    }
    if policy.callable && !policy.allow_as_subagent && !policy.allow_as_handoff_target {
        return Err(ApplicationError::ValidationError(
            "agent.profile_delegation_target_mode_required: callable profiles must allow subagent and/or handoff targeting"
                .to_string(),
        ));
    }

    let agent_list_visible = tools.allow.iter().any(|name| name == AGENT_LIST_TOOL)
        && !tools.deny.iter().any(|name| name == AGENT_LIST_TOOL);
    let agent_delegate_visible = tools.allow.iter().any(|name| name == AGENT_DELEGATE_TOOL)
        && !tools.deny.iter().any(|name| name == AGENT_DELEGATE_TOOL);
    let agent_await_visible = tools.allow.iter().any(|name| name == AGENT_AWAIT_TOOL)
        && !tools.deny.iter().any(|name| name == AGENT_AWAIT_TOOL);
    if agent_list_visible && !policy.can_delegate && !policy.can_handoff {
        return Err(ApplicationError::ValidationError(
            "agent.profile_agent_list_requires_delegation: agent.list requires delegation.canDelegate or delegation.canHandoff"
                .to_string(),
        ));
    }
    if (agent_delegate_visible || agent_await_visible) && !policy.can_delegate {
        return Err(ApplicationError::ValidationError(
            "agent.profile_agent_delegate_requires_delegation: agent.delegate/agent.await require delegation.canDelegate"
                .to_string(),
        ));
    }
    if tools.allow.iter().any(|name| name == TASK_RETURN_TOOL) {
        return Err(ApplicationError::ValidationError(
            "agent.profile_task_return_runtime_only: task.return is added by the runtime for child invocations and must not be listed in profile tools.allow"
                .to_string(),
        ));
    }

    Ok(())
}

pub(super) fn validate_run_policy(
    run: &AgentRunPolicy,
    delegation: &AgentDelegationPolicy,
    tools: &AgentToolPolicy,
) -> Result<(), ApplicationError> {
    if run.model_retry.max_retries > 0 && run.model_retry.interval_ms == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_model_retry_invalid: run.modelRetry.intervalMs must be > 0 when retries are enabled"
                .to_string(),
        ));
    }
    if !run.direct_runnable {
        if run.presentation != AgentRunPresentation::Background {
            return Err(ApplicationError::ValidationError(
                "agent.profile_subagent_only_background_required: run.presentation must be background when run.directRunnable is false"
                    .to_string(),
            ));
        }
        if !delegation.callable || !delegation.allow_as_subagent {
            return Err(ApplicationError::ValidationError(
                "agent.profile_direct_runnable_disabled_requires_subagent: run.directRunnable=false requires delegation.callable and delegation.allowAsSubagent"
                    .to_string(),
            ));
        }
        return Ok(());
    }

    if !tool_is_visible(tools, "workspace.finish") {
        return Err(ApplicationError::ValidationError(
            "agent.profile_finish_required: workspace.finish must be visible for direct runnable profiles"
                .to_string(),
        ));
    }

    if run.presentation == AgentRunPresentation::Foreground
        && !tool_is_visible(tools, "workspace.commit")
    {
        return Err(ApplicationError::ValidationError(
            "agent.profile_commit_required: foreground direct runnable profiles must expose workspace.commit"
                .to_string(),
        ));
    }

    Ok(())
}

pub(super) fn validate_skill_policy(policy: &AgentSkillPolicy) -> Result<(), ApplicationError> {
    if policy.visible.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_skill_visible_empty: skills.visible cannot be empty".to_string(),
        ));
    }
    if policy.max_read_chars_per_call == 0 || policy.max_read_chars_per_run == 0 {
        return Err(ApplicationError::ValidationError(
            "agent.profile_skill_budget_invalid: skill read budgets must be > 0".to_string(),
        ));
    }
    if policy.max_read_chars_per_call > policy.max_read_chars_per_run {
        return Err(ApplicationError::ValidationError(
            "agent.profile_skill_budget_invalid: maxReadCharsPerCall cannot exceed maxReadCharsPerRun"
                .to_string(),
        ));
    }
    for name in &policy.visible {
        if name == "*" {
            continue;
        }
        validate_skill_name(name)?;
    }
    for name in &policy.deny {
        if name == "*" {
            continue;
        }
        validate_skill_name(name)?;
    }
    Ok(())
}

pub(super) fn validate_workspace_policy(
    policy: &AgentWorkspacePolicy,
) -> Result<(), ApplicationError> {
    let universe = WORKSPACE_ROOT_UNIVERSE.into_iter().collect::<BTreeSet<_>>();
    let visible = policy
        .visible_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();
    let writable = policy
        .writable_roots
        .iter()
        .map(|root| root.as_str())
        .collect::<BTreeSet<_>>();

    if visible.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.profile_workspace_visible_empty: workspace.visibleRoots cannot be empty"
                .to_string(),
        ));
    }
    for root in visible.iter().chain(writable.iter()) {
        if !universe.contains(root) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_workspace_root_invalid: `{root}` is not an Agent workspace root"
            )));
        }
    }
    for root in &writable {
        if !visible.contains(root) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_workspace_root_invalid: writable root `{root}` is not visible"
            )));
        }
    }
    Ok(())
}

fn tool_is_visible(tools: &AgentToolPolicy, name: &str) -> bool {
    tools.allow.iter().any(|tool| tool == name) && !tools.deny.iter().any(|tool| tool == name)
}

fn validate_tool_description_override(
    spec: &AgentToolSpec,
    override_: &AgentToolDescriptionOverride,
) -> Result<(), ApplicationError> {
    if let Some(description) = override_.description.as_ref() {
        if description.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_description_empty: description for `{}` cannot be empty",
                spec.name
            )));
        }
    }
    if override_.properties.is_empty() {
        return Ok(());
    }
    let properties = spec
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.profile_tool_properties_invalid: `{}` has no object properties",
                spec.name
            ))
        })?;
    for (property, description) in &override_.properties {
        if !properties.contains_key(property) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_unknown_tool_property: `{}` has no property `{property}`",
                spec.name
            )));
        }
        if description.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_tool_property_description_empty: `{}` property `{property}` cannot be empty",
                spec.name
            )));
        }
    }
    Ok(())
}

fn validate_skill_name(name: &str) -> Result<(), ApplicationError> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        return Err(ApplicationError::ValidationError(format!(
            "agent.profile_skill_name_invalid: invalid Skill name `{name}`"
        )));
    }
    Ok(())
}
