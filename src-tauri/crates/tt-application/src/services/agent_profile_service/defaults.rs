use std::collections::BTreeMap;

use crate::errors::ApplicationError;
use tt_domain::models::agent::AgentRunPresentation;
use tt_domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy, DEFAULT_AGENT_PLAN_BETA};
use tt_domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy, AgentDelegationPolicy,
    AgentModelBinding, AgentModelBindingMode, AgentOutputArtifact, AgentOutputArtifactTarget,
    AgentOutputPolicy, AgentPresetBinding, AgentPresetBindingMode, AgentProfileDefinition,
    AgentProfileId, AgentProfileInstructions, AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy,
    AgentWorkspacePolicy, DEFAULT_AGENT_MCP_RESULT_INLINE_CHAR_LIMIT, DEFAULT_AGENT_PROFILE_ID,
    DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL, DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN,
    DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN, DEFAULT_AGENT_TOOL_MAX_ROUNDS,
};
use tt_domain::models::tool::ToolId;

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
            allow: [
                AGENT_LIST_TOOL,
                AGENT_DELEGATE_TOOL,
                AGENT_AWAIT_TOOL,
                "chat.search",
                "chat.read_messages",
                "worldinfo.read_activated",
                "skill.list",
                "skill.search",
                "skill.read",
                "skill.run_script",
                "workspace.list_files",
                "workspace.search_files",
                "workspace.read_file",
                "workspace.write_file",
                "workspace.apply_patch",
                "workspace.commit",
                "workspace.finish",
            ]
            .into_iter()
            .map(|name| {
                ToolId::builtin(name)
                    .expect("default Agent tools form valid ToolIds")
                    .to_string()
            })
            .collect(),
            deny: Vec::new(),
            tool_descriptions: BTreeMap::new(),
            max_rounds: DEFAULT_AGENT_TOOL_MAX_ROUNDS,
            max_calls_per_run: DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN,
            mcp_result_inline_char_limit: DEFAULT_AGENT_MCP_RESULT_INLINE_CHAR_LIMIT,
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
