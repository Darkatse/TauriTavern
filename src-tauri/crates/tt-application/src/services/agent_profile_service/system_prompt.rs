use crate::services::agent_tools::{
    FinishPolicy, TextTurn, WORKSPACE_FILE_TOOLS, stage_can_finish_run, visible_builtin_alias,
};
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentModelTool};

use super::constants::{
    AGENT_AWAIT_TOOL, AGENT_DELEGATE_TOOL, AGENT_HANDOFF_TOOL, TASK_RETURN_TOOL,
};

/// Where the runtime puts the workspace index in the Agent system prompt. The default
/// instructions end with it when workspace tools are enabled; custom instructions choose
/// its place, and without it they get no index.
pub(crate) const WORKSPACE_INDEX_PLACEHOLDER: &str = "{{workspace}}";

/// How the stage ends comes from its finish policy. `profile` is the invocation's, whose
/// presentation is the Run's once the invocation is prepared; root prompts are prepared
/// before the Run exists and use the Profile's default presentation.
pub fn materialize_agent_system_prompt(
    tools: &[AgentModelTool],
    profile: &ResolvedAgentProfile,
    exit_policy: AgentInvocationExitPolicy,
) -> String {
    if let Some(prompt) = profile.instructions.agent_system_prompt.as_ref() {
        return prompt.clone();
    }

    let has = |name: &str| visible_builtin_alias(tools, name).is_some();
    let alias = |name: &str| {
        visible_builtin_alias(tools, name).expect("prompt references only visible builtin tools")
    };
    if exit_policy == AgentInvocationExitPolicy::ReplyAllowed {
        let mut lines = vec![
            "Assist the user with their request. Use tools when needed and reply directly when finished."
                .to_string(),
        ];
        let uses_workspace = WORKSPACE_FILE_TOOLS.iter().any(|name| has(name));
        if uses_workspace {
            lines.push(
                "Use work/ for lasting work and tmp/ for temporary files. Both persist across turns and restarts. Remove temporary files when no longer needed."
                    .to_string(),
            );
        }
        if has("workspace.shell") {
            lines.push("Shell /work and /tmp map to work/ and tmp/ in this workspace.".to_string());
            if has("workspace.read_file")
                && (has("workspace.write_file") || has("workspace.apply_patch"))
            {
                lines.push(format!(
                    "After editing with {}, use {} before replacing or patching the file with text tools.",
                    alias("workspace.shell"),
                    alias("workspace.read_file"),
                ));
            }
        }
        if uses_workspace {
            lines.extend([String::new(), WORKSPACE_INDEX_PLACEHOLDER.to_string()]);
        }
        return lines.join("\n");
    }

    let text_turn = FinishPolicy::for_stage(
        exit_policy,
        profile.run.presentation,
        stage_can_finish_run(has),
    )
    .text_turn();
    let ends_run = text_turn != TextTurn::Continues;

    let mut lines = vec!["---".to_string(), "tools:".to_string()];
    lines.extend(
        tools
            .iter()
            .map(|tool| format!("- {}", tool.model_alias.as_str())),
    );
    lines.extend([
        "---".to_string(),
        String::new(),
        "# Agent Mode is active.".to_string(),
        "- Work with the agent tools. Tool results are working context, not chat messages."
            .to_string(),
    ]);
    lines.push(
        match (has("workspace.commit"), text_turn) {
            (true, TextTurn::EndsRun) => "- Only committed text reaches the chat. When the run's work is complete, reply without calling a tool to end the run.",
            (false, TextTurn::EndsRun) => "- When the run's work is complete, reply without calling a tool to end the run.",
            (true, _) => "- Only committed text reaches the chat; a plain-text reply is never shown to the user. Every turn must call a tool.",
            (false, _) => "- Every turn must call a tool; plain text alone does not complete this stage.",
        }
        .to_string(),
    );

    let mut completion_tools = Vec::new();
    if ends_run && has("workspace.commit") {
        completion_tools.push(format!("{} with finish: true", alias("workspace.commit")));
    }
    for name in [TASK_RETURN_TOOL, AGENT_HANDOFF_TOOL] {
        if has(name) {
            completion_tools.push(alias(name).to_string());
        }
    }
    if !completion_tools.is_empty() {
        lines.push(format!(
            "- Call {} last in its turn.",
            completion_tools.join(" or ")
        ));
    }
    if has("workspace.read_file") && (has("workspace.write_file") || has("workspace.apply_patch")) {
        lines.push(
            "- Tool results confirm writes; do not re-read a file you just wrote unless you need its content."
                .to_string(),
        );
    }

    if has("chat.search") || has("chat.read_messages") {
        let mut ways = Vec::new();
        if has("chat.search") {
            ways.push(format!("find plot by topic with {}", alias("chat.search")));
        }
        if has("chat.read_messages") {
            ways.push(format!(
                "read several floors at once with {}",
                alias("chat.read_messages")
            ));
        }
        if has("workspace.search_files") {
            ways.push(format!(
                "find exact words or a regex with {}",
                alias("workspace.search_files")
            ));
        }
        if !has("chat.read_messages") && has("workspace.read_file") {
            ways.push(format!(
                "read the original with {} floors/NNNNNN/message.md",
                alias("workspace.read_file")
            ));
        }
        lines.push(format!("- For earlier chat floors: {}.", ways.join("; ")));
    }
    if has("worldinfo.read_activated") {
        lines.push(format!(
            "- Use {} to read the World Info entries activated for this run.",
            alias("worldinfo.read_activated")
        ));
    }
    if has("dice.roll") {
        lines.push(format!(
            "- Use {} only when an explicit random roll, chance check, or tabletop/roleplay check is needed. Do not invent roll results.",
            alias("dice.roll")
        ));
    }
    if has(AGENT_DELEGATE_TOOL) {
        let await_hint = if has(AGENT_AWAIT_TOOL) {
            format!(
                "; use {} when you need a delegated result or status before deciding, and review delegated results before finalizing",
                alias(AGENT_AWAIT_TOOL)
            )
        } else {
            String::new()
        };
        lines.push(format!(
            "- Use {} to ask another Agent to handle a self-contained task. You can keep working after delegating{await_hint}.",
            alias(AGENT_DELEGATE_TOOL)
        ));
    }
    if has(AGENT_HANDOFF_TOOL) {
        lines.push(format!(
            "- Use {} when your part is done and another Agent should continue. Give a self-contained brief: objective, relevant workspace paths, decisions, constraints, and what done looks like. After it succeeds, do not call more tools.",
            alias(AGENT_HANDOFF_TOOL)
        ));
    }

    let persist_writable = profile
        .workspace
        .visible_roots
        .iter()
        .any(|root| root == "persist")
        && profile
            .workspace
            .writable_roots
            .iter()
            .any(|root| root == "persist");
    if persist_writable {
        lines.push("- Use persist/ for concise information that should carry into later turns of this chat: plot facts, unresolved threads, relationship states, user style preferences. Do not copy chat history, replies, tool results, or reasoning into it.".to_string());
    }
    // Readable and writable roots are named by the workspace index that `{{workspace}}`
    // below expands to.

    if has(TASK_RETURN_TOOL) {
        lines.push(
            "- This is a delegated task: use the workspace paths named in the task brief, the same logical paths as the requesting Agent, and write supporting notes only under writable roots."
                .to_string(),
        );
        lines.push(format!(
            "# **Important**: Return your result only by calling {} with a concise result for the requesting Agent, referencing any workspace paths you wrote.",
            alias(TASK_RETURN_TOOL)
        ));
    } else if !ends_run {
        // Only a stage that can hand off cannot finish the run itself.
        lines.push(format!(
            "# **Important**: You cannot finish the run directly. When your part is complete, call {}.",
            alias(AGENT_HANDOFF_TOOL)
        ));
    }

    if text_turn == TextTurn::EndsRunOnceCommitted
        && has("workspace.commit")
        && let Some(output) = &profile.output
    {
        let write = if has("workspace.write_file") {
            alias("workspace.write_file")
        } else {
            "write"
        };
        lines.extend([
            String::new(),
            format!(
                "# Typical flow: {write} {}{}, then {} with finish: true.",
                output.message_body_path,
                if persist_writable {
                    " (update persist/ first when needed)"
                } else {
                    ""
                },
                alias("workspace.commit")
            ),
        ]);
    }
    lines.extend([
        String::new(),
        "Anyway: TOOLS&SKILLS IS ALL YOU NEED".to_string(),
    ]);
    if WORKSPACE_FILE_TOOLS.iter().any(|name| has(name)) {
        lines.extend([String::new(), WORKSPACE_INDEX_PLACEHOLDER.to_string()]);
    }

    lines.join("\n")
}
