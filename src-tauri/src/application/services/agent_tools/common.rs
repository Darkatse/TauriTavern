use serde_json::{Map, Value, json};

use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

pub(super) fn object_args(call: &AgentToolCall) -> Option<&Map<String, Value>> {
    call.arguments.as_object()
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

pub(crate) const WORKSPACE_PATH_IS_DIRECTORY_CODE: &str = "workspace.path_is_directory";

/// Shared model-facing wording for the typed workspace-directory domain
/// error. The repository only reports the fact; this layer knows which Agent
/// tool can help the model recover.
pub(crate) fn workspace_path_is_directory_message(path: &str) -> String {
    format!(
        "workspace path `{path}` is a directory; call workspace_list_files to list its contents and re-target a specific file."
    )
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
