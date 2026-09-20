//! One-time conversion of completed v1 runs into ordinary v2 revisions.

use serde_json::Value;
use tt_domain::models::agent::profile::AGENT_PROFILE_SCHEMA_VERSION;
use tt_domain::models::agent::{AgentModelContentPart, AgentModelMessage, AgentModelRole};
use tt_domain::models::tool::{InvocationToolSnapshot, ToolSnapshotId, ToolTurnContract};

use super::super::PreparedInvocation;
use super::super::prompt_snapshot::append_runtime_catalogs_text;
use super::invalid;
use crate::errors::ApplicationError;
use crate::services::agent_profile_service::is_retired_agent_tool;
use crate::services::agent_tools::project_agent_model_tools;

pub(super) const REVISION_ONLY: &str = "older checkpoints support /fix for completed runs only; start a new run to continue unfinished work";

/// Decode the old shape for read-only inspection without upgrading its protocol.
pub(super) fn prepare_for_read(value: &mut Value) {
    if let Some(frame) = value.pointer_mut("/state/foreground") {
        remove_read_budgets(frame);
    }
    if let Some(children) = value
        .pointer_mut("/state/children")
        .and_then(Value::as_array_mut)
    {
        for frame in children {
            remove_read_budgets(frame);
        }
    }
}

fn remove_read_budgets(frame: &mut Value) {
    if let Some(skills) = frame
        .pointer_mut("/prepared/profile/skills")
        .and_then(Value::as_object_mut)
    {
        skills.remove("maxReadCharsPerCall");
        skills.remove("maxReadCharsPerRun");
    }
}

/// Called after begin_output_revision creates the new invocation and resets its
/// progress. No old execution cursor or child invocation is resumed.
pub(super) fn migrate_revision(
    prepared: &mut PreparedInvocation,
    agents: &str,
) -> Result<(), ApplicationError> {
    prepared.request.messages = convert_history(std::mem::take(&mut prepared.request.messages))?;

    let policy = &mut prepared.profile.tools;
    policy
        .allow
        .retain(|id| !is_retired_agent_tool(id.as_str()));
    policy.deny.retain(|id| !is_retired_agent_tool(id.as_str()));
    policy
        .tool_descriptions
        .retain(|id, _| !is_retired_agent_tool(id.as_str()));
    policy
        .max_calls_per_tool
        .retain(|id, _| !is_retired_agent_tool(id.as_str()));
    prepared.profile.schema_version = AGENT_PROFILE_SCHEMA_VERSION;

    // Preserve frozen aliases, descriptions and permissions, including MCP.
    // Resolving today's Profile/catalog here would change unrelated choices.
    prepared.tool_snapshot = InvocationToolSnapshot::try_new(
        ToolSnapshotId::parse(prepared.invocation.id.clone())?,
        prepared
            .tool_snapshot
            .bindings()
            .iter()
            .filter(|binding| !is_retired_agent_tool(binding.tool_id().as_str()))
            .cloned()
            .collect(),
        prepared.tool_snapshot.max_calls_per_invocation(),
    )?;
    prepared.tool_turn =
        ToolTurnContract::all(&prepared.tool_snapshot, prepared.tool_turn.choice().clone())?;
    prepared.request.tools =
        project_agent_model_tools(&prepared.tool_snapshot, &prepared.tool_turn)?;
    prepared.request.tool_choice = prepared.tool_turn.choice().clone();
    if let Some(state) = prepared.request.provider_state.as_object_mut() {
        for key in [
            "previousResponseId",
            "messageCursor",
            "lastResponseId",
            "nativeContinuation",
        ] {
            state.remove(key);
        }
    }

    // v1 did not retain PromptManager component identity. Keep its original
    // instructions/layout and add the new environment as revision context.
    let mut notice = "Revision environment: Skills are now read-only files under skills/. Read them with the available workspace tools; run scripts with workspace_shell when available. The former skill tools and agent_list are no longer available. Use the directories below to choose skills and agents. Historical execution records describe past actions and do not need to be repeated.".to_string();
    append_runtime_catalogs_text(&mut notice, &prepared.effective_skills, agents);
    prepared.request.messages.push(user_message(notice));
    Ok(())
}

fn convert_history(
    history: Vec<AgentModelMessage>,
) -> Result<Vec<AgentModelMessage>, ApplicationError> {
    let mut converted = Vec::with_capacity(history.len());
    let mut messages = history.into_iter();
    while let Some(mut message) = messages.next() {
        let has_retired_call = message.parts.iter().any(|part| {
            matches!(part, AgentModelContentPart::ToolCall { call } if is_retired_agent_tool(call.tool_id.as_str()))
        });
        if !has_retired_call {
            converted.push(message);
            continue;
        }

        // Runtime records each tool turn as one assistant message followed by
        // its ordered results. Convert the whole turn, including native metadata,
        // so mixed retired/current calls cannot leave dangling protocol records.
        let mut record = "Historical execution record from the previous run. Context for revision; do not repeat these actions.".to_string();
        for part in &message.parts {
            let AgentModelContentPart::ToolCall { call } = part else {
                continue;
            };
            let result_message = messages.next().ok_or_else(|| {
                invalid(format!("missing historical result for {}", call.call_id))
            })?;
            let result = match result_message.parts.as_slice() {
                [AgentModelContentPart::ToolResult { result }]
                    if result_message.role == AgentModelRole::Tool
                        && result.call_id == call.call_id
                        && result.tool_id == call.tool_id =>
                {
                    result
                }
                _ => {
                    return Err(invalid(format!(
                        "historical result does not match {}",
                        call.call_id
                    )));
                }
            };
            let arguments =
                serde_json::to_string(&call.arguments).expect("tool arguments are JSON");
            record.push_str(&format!(
                "\n\nTool: {}\nCall: {}\nArguments: {arguments}\nStatus: {}\nResult:\n{}",
                call.tool_id,
                call.call_id,
                if result.is_error { "error" } else { "success" },
                result.content
            ));
        }
        message.parts.retain(|part| {
            matches!(
                part,
                AgentModelContentPart::Text { .. }
                    | AgentModelContentPart::Media { .. }
                    | AgentModelContentPart::ResourceRef { .. }
            )
        });
        message.provider_metadata = Value::Null;
        if !message.parts.is_empty() {
            converted.push(message);
        }
        converted.push(user_message(record));
    }
    Ok(converted)
}

fn user_message(text: String) -> AgentModelMessage {
    AgentModelMessage {
        role: AgentModelRole::User,
        parts: vec![AgentModelContentPart::Text { text }],
        provider_metadata: Value::Null,
    }
}
