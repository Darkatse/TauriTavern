use serde_json::{Map, Value};

use super::policy::WorkspaceAccessPolicy;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};

pub(super) use crate::application::services::agent_tools::common::{
    object_args, optional_bool_arg, optional_usize_arg, required_raw_string_arg,
    required_trimmed_string_arg, tool_error,
};

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
