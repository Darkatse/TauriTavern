use serde_json::{Map, Value, json};

use super::policy::WorkspaceAccessPolicy;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};

pub(super) fn object_args(call: &AgentToolCall) -> Option<&Map<String, Value>> {
    call.arguments.as_object()
}

pub(super) fn optional_list_path_arg(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<WorkspacePath>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    let Some(raw) = value.as_str() else {
        return Err(format!("{key} must be a string"));
    };
    let value = raw.trim();
    if value.is_empty() || value == "." || value == "./" {
        return Ok(None);
    }

    WorkspacePath::parse(value)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(super) fn parse_workspace_path(
    call: &AgentToolCall,
    raw: &str,
) -> Result<WorkspacePath, AgentToolResult> {
    WorkspacePath::parse(raw)
        .map_err(|error| tool_error(call, "workspace.invalid_path", &error.to_string()))
}

pub(super) fn ensure_visible_workspace_path(
    call: &AgentToolCall,
    policy: &WorkspaceAccessPolicy,
    path: &WorkspacePath,
) -> Result<(), AgentToolResult> {
    policy
        .ensure_visible(path)
        .map_err(|error| tool_error(call, "workspace.path_not_visible", &error.to_string()))
}

pub(super) fn ensure_writable_workspace_path(
    call: &AgentToolCall,
    policy: &WorkspaceAccessPolicy,
    path: &WorkspacePath,
) -> Result<(), AgentToolResult> {
    policy
        .ensure_writable(path)
        .map_err(|error| tool_error(call, "workspace.path_not_writable", &error.to_string()))
}

pub(super) fn required_trimmed_string_arg<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn required_raw_string_arg<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub(super) fn optional_usize_arg(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<usize>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let Some(value) = value.as_u64() else {
        return Err(format!("{key} must be a non-negative integer"));
    };
    usize::try_from(value)
        .map(Some)
        .map_err(|_| format!("{key} is too large"))
}

pub(super) fn optional_bool_arg(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Option<bool>, String> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("{key} must be a boolean"))
}

pub(super) fn tool_error(call: &AgentToolCall, error_code: &str, message: &str) -> AgentToolResult {
    AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: message.to_string(),
        structured: json!({
            "error": {
                "code": error_code,
                "message": message,
            }
        }),
        is_error: true,
        error_code: Some(error_code.to_string()),
        resource_refs: Vec::new(),
    }
}
