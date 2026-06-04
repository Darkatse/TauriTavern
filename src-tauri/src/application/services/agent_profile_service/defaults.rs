use std::collections::BTreeMap;

use crate::application::errors::ApplicationError;
use crate::domain::models::agent::AgentRunPresentation;
use crate::domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy, DEFAULT_AGENT_PLAN_BETA};
use crate::domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy, AgentDelegationPolicy,
    AgentModelBinding, AgentModelBindingMode, AgentOutputArtifact, AgentOutputArtifactTarget,
    AgentOutputPolicy, AgentPresetBinding, AgentPresetBindingMode, AgentProfileDefinition,
    AgentProfileId, AgentProfileInstructions, AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy,
    AgentWorkspacePolicy, DEFAULT_AGENT_PROFILE_ID, DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL,
    DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN, DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN,
    DEFAULT_AGENT_TOOL_MAX_ROUNDS,
};

use super::constants::{
    AGENT_AWAIT_TOOL, AGENT_DELEGATE_TOOL, AGENT_LIST_TOOL, WORKSPACE_ROOT_UNIVERSE,
};

pub(super) fn default_writer_profile() -> Result<AgentProfileDefinition, ApplicationError> {
    Ok(AgentProfileDefinition {
        schema_version: AGENT_PROFILE_SCHEMA_VERSION,
        kind: AGENT_PROFILE_KIND.to_string(),
        id: AgentProfileId::parse(DEFAULT_AGENT_PROFILE_ID)
            .map_err(ApplicationError::ValidationError)?,
        display_name: "Default Writer".to_string(),
        description: Some("General creative writing Agent profile.".to_string()),
        preset: AgentPresetBinding {
            mode: AgentPresetBindingMode::CurrentPromptSnapshot,
            ref_: None,
            required: false,
        },
        model: AgentModelBinding {
            mode: AgentModelBindingMode::CurrentPromptSnapshot,
            connection_ref: None,
            model_id: None,
        },
        run: AgentRunPolicy {
            presentation: AgentRunPresentation::Foreground,
            direct_runnable: true,
            model_retry: Default::default(),
        },
        context: AgentContextPolicy::default(),
        instructions: AgentProfileInstructions {
            agent_system_prompt: None,
        },
        delegation: AgentDelegationPolicy {
            can_delegate: true,
            ..Default::default()
        },
        tools: AgentToolPolicy {
            allow: vec![
                AGENT_LIST_TOOL.to_string(),
                AGENT_DELEGATE_TOOL.to_string(),
                AGENT_AWAIT_TOOL.to_string(),
                "chat.search".to_string(),
                "chat.read_messages".to_string(),
                "worldinfo.read_activated".to_string(),
                "skill.list".to_string(),
                "skill.search".to_string(),
                "skill.read".to_string(),
                "workspace.list_files".to_string(),
                "workspace.search_files".to_string(),
                "workspace.read_file".to_string(),
                "workspace.write_file".to_string(),
                "workspace.apply_patch".to_string(),
                "workspace.commit".to_string(),
                "workspace.finish".to_string(),
            ],
            deny: Vec::new(),
            tool_descriptions: BTreeMap::new(),
            max_rounds: DEFAULT_AGENT_TOOL_MAX_ROUNDS,
            max_calls_per_run: DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN,
            max_calls_per_tool: BTreeMap::new(),
        },
        skills: AgentSkillPolicy {
            visible: vec!["*".to_string()],
            deny: Vec::new(),
            max_read_chars_per_call: DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL,
            max_read_chars_per_run: DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN,
        },
        workspace: AgentWorkspacePolicy {
            visible_roots: WORKSPACE_ROOT_UNIVERSE
                .iter()
                .map(|root| root.to_string())
                .collect(),
            writable_roots: WORKSPACE_ROOT_UNIVERSE
                .iter()
                .map(|root| root.to_string())
                .collect(),
        },
        plan: AgentPlanPolicy {
            mode: AgentPlanMode::None,
            beta: DEFAULT_AGENT_PLAN_BETA,
            nodes: Vec::new(),
        },
        output: AgentOutputPolicy {
            artifacts: vec![AgentOutputArtifact {
                id: "main".to_string(),
                path: "output/main.md".to_string(),
                kind: "markdown".to_string(),
                target: AgentOutputArtifactTarget::MessageBody,
                required: true,
                assembly_order: 0,
            }],
        },
    })
}
