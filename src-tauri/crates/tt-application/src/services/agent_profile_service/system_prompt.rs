use crate::services::agent_workspace_scope::{
    format_model_visible_workspace_roots, format_model_workspace_roots,
};
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentModelTool};

use super::constants::{
    AGENT_AWAIT_TOOL, AGENT_DELEGATE_TOOL, AGENT_HANDOFF_TOOL, TASK_RETURN_TOOL,
};

pub fn materialize_agent_system_prompt(
    tools: &[AgentModelTool],
    profile: &ResolvedAgentProfile,
    exit_policy: AgentInvocationExitPolicy,
) -> String {
    if let Some(prompt) = profile.instructions.agent_system_prompt.as_ref() {
        return prompt.clone();
    }

    if exit_policy == AgentInvocationExitPolicy::ReplyAllowed {
        return format!(
            "You are an assistant working in a continuous Session. Use tools when useful and reply directly to the user when finished.\nReadable workspace directories: {}.\nWritable workspace directories: {}.\nwork/ holds lasting work. tmp/ holds disposable intermediate files; both persist across turns and restarts. Clean up temporary files when they are no longer needed. Workspace text tools and Shell share the same files; Shell /work and /tmp map to work/ and tmp/. After Shell edits, read the file before replacing or patching it with text tools.",
            format_model_visible_workspace_roots(&profile.workspace.visible_roots),
            format_model_workspace_roots(&profile.workspace.writable_roots),
        );
    }

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
        "- Work using the available agent tools. Tool results are working context, not chat messages.".to_string(),
        "- Every model turn must include at least one Agent tool call. Plain text alone does not complete the current stage.".to_string(),
        String::new(),
    ]);

    if has_tool(tools, "chat.search") {
        lines.push(format!(
            "- When more context is needed, use {} to find relevant prior messages. Provide only the search query.",
            model_alias(tools, "chat.search")
        ));
    }
    if has_tool(tools, "chat.read_messages") {
        let source_hint = if has_tool(tools, "chat.search") {
            format!(
                "the message indices returned by {}",
                model_alias(tools, "chat.search")
            )
        } else {
            "exact indexes you already know".to_string()
        };
        lines.push(format!(
            "- Use {} with {source_hint} for review. Messages are read in full by default; when a preview is returned, continue with start_line and line_count.",
            model_alias(tools, "chat.read_messages")
        ));
    }
    if has_tool(tools, "worldinfo.read_activated") {
        lines.push(format!(
            "- When activated world information is relevant to this run, use {}.",
            model_alias(tools, "worldinfo.read_activated")
        ));
    }
    if has_tool(tools, "dice.roll") {
        lines.push(format!(
            "- Use {} only when an explicit random roll, chance check, or tabletop/roleplay check is needed. Do not invent roll results.",
            model_alias(tools, "dice.roll")
        ));
    }
    if has_tool(tools, AGENT_DELEGATE_TOOL) {
        if has_tool(tools, AGENT_AWAIT_TOOL) {
            lines.push(format!(
                "- Use {} to ask another Agent to handle a self-contained task. You can continue working after delegating; use {} when you need a delegated result or status before deciding.",
                model_alias(tools, AGENT_DELEGATE_TOOL),
                model_alias(tools, AGENT_AWAIT_TOOL)
            ));
            lines.push(
                "- If delegated task results are provided later, review them before finalizing."
                    .to_string(),
            );
        } else {
            lines.push(format!(
                "- Use {} to ask another Agent to handle a self-contained task. You can continue working after delegating.",
                model_alias(tools, AGENT_DELEGATE_TOOL)
            ));
        }
    }
    if has_tool(tools, AGENT_HANDOFF_TOOL) {
        lines.push(format!(
            "- Use {} when you have finished your part and another Agent should continue. Provide a self-contained handoff brief with the objective, relevant workspace paths, decisions, constraints, and what done looks like.",
            model_alias(tools, AGENT_HANDOFF_TOOL)
        ));
        lines.push(format!(
            "- After {} succeeds, your part is done; do not call more tools.",
            model_alias(tools, AGENT_HANDOFF_TOOL)
        ));
    }
    if has_tool(tools, "workspace.shell") {
        lines.push(
            "- Workspace tools share the same files. Each shell call starts a new session; files persist between calls."
                .to_string(),
        );
        if has_tool(tools, "workspace.read_file")
            && (has_tool(tools, "workspace.apply_patch") || has_tool(tools, "workspace.write_file"))
        {
            lines.push(format!(
                "- After editing a file with {}, read it with {} before patching or replacing it with the text tools.",
                model_alias(tools, "workspace.shell"),
                model_alias(tools, "workspace.read_file")
            ));
        }
    }
    if let Some(output) = &profile.output
        && has_tool(tools, "workspace.commit")
    {
        lines.push(format!(
            "- Use {} to publish Run workspace files into the current chat message. Without arguments, it will replace the current run's chat message with {}; mode append will append to the same message, creating it if this run has not committed yet.",
            model_alias(tools, "workspace.commit"),
            output.message_body_path
        ));
    }

    if profile
        .workspace
        .visible_roots
        .iter()
        .any(|root| root == "persist")
        && profile
            .workspace
            .writable_roots
            .iter()
            .any(|root| root == "persist")
    {
        lines.push("- Use persist/ to store concise information that should carry over into subsequent runs of the same chat, such as persistent plot facts, unresolved threads, relationship states, and user style preferences.".to_string());
        lines.push(
            "- **Do not** copy full chat history, final replies, tool results, or temporary reasoning into persist/."
                .to_string(),
        );
    }

    if has_tool(tools, TASK_RETURN_TOOL) {
        lines.push(
            "- Delegated task workspace: use the same logical workspace paths as the Agent that asked for this task. Do not invent private path mappings."
                .to_string(),
        );
        lines.push(
            "- Use the workspace paths named in the task brief. Write supporting notes or artifacts only under writable roots."
                .to_string(),
        );
        lines.push(format!(
            "- Readable workspace directories: {}.",
            format_model_visible_workspace_roots(&profile.workspace.visible_roots)
        ));
        lines.push(format!(
            "- Writable workspace directories: {}.",
            format_model_workspace_roots(&profile.workspace.writable_roots)
        ));
        lines.push(format!(
            "# **Important**: You are completing a delegated task. Return your result only by calling {} with a concise result for the requesting Agent.",
            model_alias(tools, TASK_RETURN_TOOL)
        ));
        lines.push(
            "- If useful, write supporting notes or requested artifacts, then reference those workspace paths in task_return."
                .to_string(),
        );
    } else {
        lines.push(format!(
            "- Readable workspace directories: {}.",
            format_model_visible_workspace_roots(&profile.workspace.visible_roots)
        ));
        lines.push(format!(
            "- Writable workspace directories: {}.",
            format_model_workspace_roots(&profile.workspace.writable_roots)
        ));
        lines.push(
            "> You may encounter: \"No visible workspace files found.\" This happens because there are no persisted files; please continue."
                .to_string(),
        );
        match (
            profile.run.presentation,
            has_tool(tools, "workspace.finish"),
            has_tool(tools, "workspace.commit"),
            has_tool(tools, AGENT_HANDOFF_TOOL),
        ) {
            (
                tt_domain::models::agent::AgentRunPresentation::Foreground,
                true,
                true,
                _,
            ) => lines.push(format!(
                "# **Important**: Before calling {}, you **must successfully call {} at least once** so that the user can see the final chat message.",
                model_alias(tools, "workspace.finish"),
                model_alias(tools, "workspace.commit")
            )),
            (
                tt_domain::models::agent::AgentRunPresentation::Foreground,
                true,
                false,
                _,
            ) => lines.push(format!(
                "# **Important**: Call {} only when this foreground stage can end without you publishing a new chat commit.",
                model_alias(tools, "workspace.finish")
            )),
            (tt_domain::models::agent::AgentRunPresentation::Background, true, _, _) => {
                lines.push(format!(
                    "# Background runs may call {} without committing a chat message.",
                    model_alias(tools, "workspace.finish")
                ));
            }
            (_, false, _, true) => lines.push(format!(
                "# **Important**: You cannot finish the run directly with the available tools. When your part is complete, call {}.",
                model_alias(tools, AGENT_HANDOFF_TOOL)
            )),
            (_, false, _, false) => lines.push(
                "# **Important**: You do not have a finish or handoff tool. Use another available Agent tool to move the work forward."
                    .to_string(),
            ),
        }
        if has_tool(tools, "workspace.finish") {
            lines.push(format!(
                "# **Important**: Do not answer in plain text. Finish by calling {}.",
                model_alias(tools, "workspace.finish")
            ));
        } else if has_tool(tools, AGENT_HANDOFF_TOOL) {
            lines.push(format!(
                "# **Important**: Do not answer in plain text. Continue by calling {}.",
                model_alias(tools, AGENT_HANDOFF_TOOL)
            ));
        }
    }
    if has_tool(tools, "workspace.commit") && has_tool(tools, "workspace.finish") {
        lines.extend([
            String::new(),
            format!(
                "# Basic tool calling flow (adjusted based on the actual situation, but the flow must include {} + {}):",
                model_alias(tools, "workspace.commit"),
                model_alias(tools, "workspace.finish")
            ),
            String::new(),
            "A simple template you can follow:".to_string(),
            "    (thoughts before actions)".to_string(),
            "    (call tools)(optional)".to_string(),
            String::new(),
            format!(
                "    Now I need to call \"{}\" once.",
                model_alias(tools, "workspace.commit")
            ),
            format!(
                "    Good, it has been committed. Finally, don't forget to call \"{}\".",
                model_alias(tools, "workspace.finish")
            ),
            String::new(),
            "You also can follow commit-N-times template:".to_string(),
            "    (thoughts before actions)".to_string(),
        ]);
        if has_tool(tools, "workspace.read_file") {
            lines.push(format!(
                "    ({})",
                model_alias(tools, "workspace.read_file")
            ));
        }
        if has_tool(tools, "worldinfo.read_activated") {
            lines.push(format!(
                "    ({})",
                model_alias(tools, "worldinfo.read_activated")
            ));
        }
        lines.extend([
            format!(
                "    (call {} with append mode)",
                model_alias(tools, "workspace.commit")
            ),
            "    (think)".to_string(),
            "    (edit if necessary)".to_string(),
            format!(
                "    ({} with append mode)",
                model_alias(tools, "workspace.commit")
            ),
            String::new(),
        ]);
    }
    lines.push("Anyway: TOOLS&SKILLS IS ALL YOU NEED".to_string());

    lines.join("\n")
}

fn has_tool(tools: &[AgentModelTool], name: &str) -> bool {
    tools
        .iter()
        .any(|tool| tool.tool_id.is_builtin() && tool.tool_id.native_name() == name)
}

fn model_alias<'a>(tools: &'a [AgentModelTool], name: &'a str) -> &'a str {
    tools
        .iter()
        .find(|tool| tool.tool_id.is_builtin() && tool.tool_id.native_name() == name)
        .map(|tool| tool.model_alias.as_str())
        .expect("prompt references only visible builtin tools")
}
