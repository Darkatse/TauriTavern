use serde_json::{Map, Value};

use super::policy::WorkspaceAccessPolicy;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};

pub(super) use crate::services::agent_tools::common::{
    WORKSPACE_PATH_IS_DIRECTORY_CODE, object_args, optional_bool_arg, optional_usize_arg,
    required_raw_string_arg, required_trimmed_string_arg, tool_error,
    workspace_path_is_directory_message,
};

pub(super) struct WorkspaceToolError {
    code: &'static str,
    message: String,
}

impl WorkspaceToolError {
    pub(super) fn into_tool_result(self, call: &AgentToolCall) -> AgentToolResult {
        tool_error(call, self.code, &self.message)
    }
}

/// Pattern-match helper used by every read/write entry point so they all
/// surface filesystem errors raised by the workspace repository as
/// recoverable model-facing tool errors instead of bubbling up as
/// `agent.internal_error`.
pub(super) fn classify_workspace_io_error(
    call: &AgentToolCall,
    error: DomainError,
) -> Result<AgentToolResult, DomainError> {
    match error {
        DomainError::NotFound(message) => {
            Ok(tool_error(call, "workspace.file_not_found", &message))
        }
        DomainError::WorkspacePathIsDirectory { path } => Ok(tool_error(
            call,
            WORKSPACE_PATH_IS_DIRECTORY_CODE,
            &workspace_path_is_directory_message(&path),
        )),
        other => Err(other),
    }
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

pub(super) fn parse_workspace_path(raw: &str) -> Result<WorkspacePath, WorkspaceToolError> {
    WorkspacePath::parse(raw).map_err(|error| WorkspaceToolError {
        code: "workspace.invalid_path",
        message: error.to_string(),
    })
}

pub(super) fn ensure_visible_workspace_path(
    policy: &WorkspaceAccessPolicy,
    path: &WorkspacePath,
) -> Result<(), WorkspaceToolError> {
    policy
        .ensure_visible(path)
        .map_err(|error| WorkspaceToolError {
            code: "workspace.path_not_visible",
            message: error.to_string(),
        })
}

pub(super) fn ensure_writable_workspace_path(
    policy: &WorkspaceAccessPolicy,
    path: &WorkspacePath,
) -> Result<(), WorkspaceToolError> {
    policy
        .ensure_writable(path)
        .map_err(|error| WorkspaceToolError {
            code: "workspace.path_not_writable",
            message: error.to_string(),
        })
}
