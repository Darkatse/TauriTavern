use crate::services::agent_workspace_scope::format_model_workspace_roots;
use tt_domain::models::agent::AgentToolSpec;
use tt_domain::models::agent::profile::ResolvedAgentProfile;

use super::constants::{
    AGENT_AWAIT_TOOL, AGENT_DELEGATE_TOOL, AGENT_HANDOFF_TOOL, AGENT_LIST_TOOL, TASK_RETURN_TOOL,
};

pub fn materialize_agent_system_prompt(
    tools: &[AgentToolSpec],
    profile: &ResolvedAgentProfile,
) -> String {
    if let Some(prompt) = profile.instructions.agent_system_prompt.as_ref() {
        return prompt.clone();
    }

    let mut lines = vec![
        "---".to_string(),
        "tool_choice: required".to_string(),
        "tools:".to_string(),
    ];
    lines.extend(
        tools
            .iter()
            .map(|tool| format!("- {}", tool.model_name.as_str())),
    );
    lines.extend([
        "---".to_string(),
        String::new(),
        "# Agent Mode is active.".to_string(),
        "- Work using the available agent tools. Tool results are working context, not chat messages.".to_string(),
        String::new(),
    ]);

    if has_tool(tools, "chat.search") {
        lines.push(format!(
            "- When more context is needed, use {} to find relevant prior messages. Provide only the search query.",
            model_name(tools, "chat.search")
        ));
    }
    if has_tool(tools, "chat.read_messages") {
        let source_hint = if has_tool(tools, "chat.search") {
            format!(
                "the message indices returned by {}",
                model_name(tools, "chat.search")
            )
        } else {
            "exact indexes you already know".to_string()
        };
        lines.push(format!(
            "- Use {} with {source_hint} for review. For longer messages, use start_char and max_chars to read smaller ranges.",
            model_name(tools, "chat.read_messages")
        ));
    }
    if has_tool(tools, "worldinfo.read_activated") {
        lines.push(format!(
            "- When activated world information is relevant to this run, use {}.",
            model_name(tools, "worldinfo.read_activated")
        ));
    }
    if has_tool(tools, "dice.roll") {
        lines.push(format!(
            "- Use {} only when an explicit random roll, chance check, or tabletop/roleplay check is needed. Do not invent roll results.",
            model_name(tools, "dice.roll")
        ));
    }
    if has_tool(tools, "skill.list") {
        lines.push(format!(
            "- Use {} to discover visible agent skills when reusable writing, editing, planning, style, or character guidance may be helpful.",
            model_name(tools, "skill.list")
        ));
    }
    if has_tool(tools, AGENT_LIST_TOOL) {
        lines.push(format!(
            "- Use {} to find other Agents that can help with a focused writing, critique, planning, or style task. This tool only lists Agents; it does not start any work.",
            model_name(tools, AGENT_LIST_TOOL)
        ));
    }
    if has_tool(tools, AGENT_DELEGATE_TOOL) {
        if has_tool(tools, AGENT_AWAIT_TOOL) {
            lines.push(format!(
                "- Use {} to ask another Agent to handle a self-contained task. You can continue working after delegating; use {} when you need a delegated result or status before deciding.",
                model_name(tools, AGENT_DELEGATE_TOOL),
                model_name(tools, AGENT_AWAIT_TOOL)
            ));
            lines.push(
                "- If delegated task results are provided later, review them before finalizing."
                    .to_string(),
            );
        } else {
            lines.push(format!(
                "- Use {} to ask another Agent to handle a self-contained task. You can continue working after delegating.",
                model_name(tools, AGENT_DELEGATE_TOOL)
            ));
        }
    }
    if has_tool(tools, AGENT_HANDOFF_TOOL) {
        lines.push(format!(
            "- Use {} when you have finished your part and another Agent should continue. Provide a self-contained handoff brief with the objective, relevant workspace paths, decisions, constraints, and what done looks like.",
            model_name(tools, AGENT_HANDOFF_TOOL)
        ));
        lines.push(format!(
            "- After {} succeeds, your part is done; do not call more tools.",
            model_name(tools, AGENT_HANDOFF_TOOL)
        ));
    }
    if has_tool(tools, "skill.search") {
        lines.push(format!(
            "- Before reading exact ranges, use {} to locate relevant text within larger visible skill files.",
            model_name(tools, "skill.search")
        ));
    }
    if has_tool(tools, "skill.read") {
        lines.push(format!(
            "- Use {} to read SKILL.md first, then only read referenced skill files or specified ranges within them when necessary.",
            model_name(tools, "skill.read")
        ));
    }
    if has_tool(tools, "workspace.list_files") {
        lines.push(format!(
            "- Use {} to inspect visible workspace files.",
            model_name(tools, "workspace.list_files")
        ));
    }
    if has_tool(tools, "workspace.search_files") {
        lines.push(format!(
            "- Before reading exact ranges, use {} to find relevant text within visible workspace files (e.g., persist/ memory).",
            model_name(tools, "workspace.search_files")
        ));
    }
    if has_tool(tools, "workspace.read_file") {
        lines.push(format!(
            "- Use {} before modifying an existing file. Read the exact text you want to replace; if a patch fails, fully read the file before retrying. Read content includes line numbers; never include line number prefixes in old_string or new_string.",
            model_name(tools, "workspace.read_file")
        ));
    }
    if has_tool(tools, "workspace.apply_patch") {
        lines.push(format!(
            "- Use {} to perform precise edits on existing files. old_string must match exactly and be unique unless replace_all is true.",
            model_name(tools, "workspace.apply_patch")
        ));
    }
    if has_tool(tools, "workspace.write_file") {
        lines.push(format!(
            "- Use {} to create files, append to files, or perform complete rewrites.",
            model_name(tools, "workspace.write_file")
        ));
    }
    if has_tool(tools, "workspace.commit") {
        lines.push(format!(
            "- Use {} to publish visible workspace files into the current chat message. Without arguments, it will replace the current run's chat message with {}; mode append will append to the same message, creating it if this run has not committed yet.",
            model_name(tools, "workspace.commit"),
            profile.output.message_body_path
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
            "- Visible workspace roots for this task: {}.",
            format_model_workspace_roots(&profile.workspace.visible_roots)
        ));
        lines.push(format!(
            "- Writable workspace roots for this task: {}.",
            format_model_workspace_roots(&profile.workspace.writable_roots)
        ));
        lines.push(format!(
            "# **Important**: You are completing a delegated task. Return your result only by calling {} with a concise result for the requesting Agent.",
            model_name(tools, TASK_RETURN_TOOL)
        ));
        lines.push(
            "- If useful, write supporting notes or requested artifacts, then reference those workspace paths in task_return."
                .to_string(),
        );
    } else {
        lines.push(format!(
            "- Visible workspace roots: {}.",
            format_model_workspace_roots(&profile.workspace.visible_roots)
        ));
        lines.push(format!(
            "- Writable workspace roots: {}.",
            format_model_workspace_roots(&profile.workspace.writable_roots)
        ));
        lines.push(format!(
            "- **Never** read {} before commit",
            profile.output.message_body_path
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
                model_name(tools, "workspace.finish"),
                model_name(tools, "workspace.commit")
            )),
            (
                tt_domain::models::agent::AgentRunPresentation::Foreground,
                true,
                false,
                _,
            ) => lines.push(format!(
                "# **Important**: Call {} only when this foreground stage can end without you publishing a new chat commit.",
                model_name(tools, "workspace.finish")
            )),
            (tt_domain::models::agent::AgentRunPresentation::Background, true, _, _) => {
                lines.push(format!(
                    "# Background runs may call {} without committing a chat message.",
                    model_name(tools, "workspace.finish")
                ));
            }
            (_, false, _, true) => lines.push(format!(
                "# **Important**: You cannot finish the run directly with the available tools. When your part is complete, call {}.",
                model_name(tools, AGENT_HANDOFF_TOOL)
            )),
            (_, false, _, false) => lines.push(
                "# **Important**: You do not have a finish or handoff tool. Use another available Agent tool to move the work forward."
                    .to_string(),
            ),
        }
        if has_tool(tools, "workspace.finish") {
            lines.push(format!(
                "# **Important**: Do not answer in plain text. Finish by calling {}.",
                model_name(tools, "workspace.finish")
            ));
        } else if has_tool(tools, AGENT_HANDOFF_TOOL) {
            lines.push(format!(
                "# **Important**: Do not answer in plain text. Continue by calling {}.",
                model_name(tools, AGENT_HANDOFF_TOOL)
            ));
        }
    }
    if has_tool(tools, "workspace.commit") && has_tool(tools, "workspace.finish") {
        lines.extend([
            String::new(),
            format!(
                "# Basic tool calling flow (adjusted based on the actual situation, but the flow must include {} + {}):",
                model_name(tools, "workspace.commit"),
                model_name(tools, "workspace.finish")
            ),
            String::new(),
            "A simple template you can follow:".to_string(),
            "    (thoughts before actions)".to_string(),
            "    (call tools)(optional)".to_string(),
            String::new(),
            format!(
                "    Now I need to call \"{}\" once.",
                model_name(tools, "workspace.commit")
            ),
            format!(
                "    Good, it has been committed. Finally, don't forget to call \"{}\".",
                model_name(tools, "workspace.finish")
            ),
            String::new(),
            "You also can follow commit-N-times template:".to_string(),
            "    (thoughts before actions)".to_string(),
        ]);
        if has_tool(tools, "workspace.read_file") {
            lines.push(format!(
                "    ({})",
                model_name(tools, "workspace.read_file")
            ));
        }
        if has_tool(tools, "worldinfo.read_activated") {
            lines.push(format!(
                "    ({})",
                model_name(tools, "worldinfo.read_activated")
            ));
        }
        if has_tool(tools, "skill.list") {
            lines.push(format!("    ({})", model_name(tools, "skill.list")));
        }
        lines.extend([
            format!(
                "    (call {} with append mode)",
                model_name(tools, "workspace.commit")
            ),
            "    (think)".to_string(),
            "    (edit if necessary)".to_string(),
            format!(
                "    ({} with append mode)",
                model_name(tools, "workspace.commit")
            ),
            String::new(),
        ]);
    }
    lines.push("Anyway: TOOLS&SKILLS IS ALL YOU NEED".to_string());

    lines.join("\n")
}

fn has_tool(tools: &[AgentToolSpec], name: &str) -> bool {
    tools.iter().any(|tool| tool.name == name)
}

fn model_name<'a>(tools: &'a [AgentToolSpec], name: &'a str) -> &'a str {
    tools
        .iter()
        .find(|tool| tool.name == name)
        .map(|tool| tool.model_name.as_str())
        .unwrap_or(name)
}
