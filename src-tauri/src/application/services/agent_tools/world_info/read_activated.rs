use serde_json::{Value, json};

use super::{DEFAULT_WORLDINFO_MAX_CHARS, MAX_WORLDINFO_CHARS};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::common::{
    object_args, optional_usize_arg, tool_error,
};
use crate::application::services::agent_tools::dispatcher::AgentToolEffect;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

pub(in crate::application::services::agent_tools) async fn read_activated(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let max_chars = match optional_usize_arg(args, "max_chars") {
        Ok(value) => value.unwrap_or(DEFAULT_WORLDINFO_MAX_CHARS),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if max_chars == 0 {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "max_chars must be >= 1"),
            AgentToolEffect::None,
        ));
    }
    if max_chars > MAX_WORLDINFO_CHARS {
        return Ok((
            tool_error(
                call,
                "worldinfo.max_chars_too_large",
                &format!("max_chars must be <= {MAX_WORLDINFO_CHARS}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let snapshot_path = WorkspacePath::parse("input/prompt_snapshot.json")?;
    let snapshot_file = workspace_repository
        .read_text(run_id, &snapshot_path)
        .await
        .map_err(ApplicationError::from)?;
    let snapshot: Value = serde_json::from_str(&snapshot_file.text).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "agent.invalid_prompt_snapshot_file: failed to parse prompt snapshot JSON: {error}"
        ))
    })?;

    let Some(batch) = snapshot.get("worldInfoActivation") else {
        return Ok((
            tool_error(
                call,
                "worldinfo.activation_unavailable",
                "this run has no worldInfoActivation snapshot",
            ),
            AgentToolEffect::None,
        ));
    };
    let entries = batch
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.invalid_worldinfo_activation_snapshot: entries must be an array".to_string(),
            )
        })?;

    let content_chars = entries
        .iter()
        .map(|entry| entry.get("content").and_then(Value::as_str).unwrap_or(""))
        .map(|content| content.chars().count())
        .sum::<usize>();
    if content_chars > max_chars {
        return Ok((
            tool_error(
                call,
                "worldinfo.result_too_large",
                &format!(
                    "activated World Info has {content_chars} content characters; call with a larger max_chars up to {MAX_WORLDINFO_CHARS}"
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let normalized_entries = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| normalize_entry(index, entry))
        .collect::<Vec<_>>();
    let resource_refs = normalized_entries
        .iter()
        .map(|entry| entry["ref"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    let content = render_content(&normalized_entries);

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "timestampMs": batch.get("timestampMs").and_then(Value::as_i64),
                "trigger": batch.get("trigger").and_then(Value::as_str),
                "entries": normalized_entries,
            }),
            is_error: false,
            error_code: None,
            resource_refs,
        },
        AgentToolEffect::None,
    ))
}

fn normalize_entry(index: usize, entry: &Value) -> Value {
    let world = entry.get("world").and_then(Value::as_str).unwrap_or("");
    let uid = match entry.get("uid") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    };
    let ref_id = if world.is_empty() || uid.is_empty() {
        format!("worldinfo:activated#{index}")
    } else {
        format!("worldinfo:{world}#{uid}")
    };

    json!({
        "world": world,
        "uid": uid,
        "displayName": entry.get("displayName").and_then(Value::as_str),
        "constant": entry.get("constant").and_then(Value::as_bool).unwrap_or(false),
        "position": entry.get("position").and_then(Value::as_str),
        "content": entry.get("content").and_then(Value::as_str).unwrap_or(""),
        "ref": ref_id,
    })
}

fn render_content(entries: &[Value]) -> String {
    if entries.is_empty() {
        return "No World Info entries were activated for this run.".to_string();
    }

    let mut content = format!(
        "Activated World Info for this run: {} entr{}.",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" }
    );
    for entry in entries {
        let world = entry.get("world").and_then(Value::as_str).unwrap_or("");
        let display_name = entry
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("");
        let entry_content = entry.get("content").and_then(Value::as_str).unwrap_or("");

        content.push_str("\n\n[");
        content.push_str(display_name);
        content.push_str(" | ");
        content.push_str(world);
        content.push_str("]\n");
        content.push_str(entry_content);
    }
    content
}
