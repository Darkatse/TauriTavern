use serde_json::json;

use super::args::{
    ensure_writable_workspace_path, object_args, optional_bool_arg, parse_workspace_path,
    required_raw_string_arg, required_trimmed_string_arg, tool_error,
};
use super::policy::workspace_access_policy;
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;

pub(in crate::application::services::agent_tools) async fn apply_patch(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
    session: &mut AgentToolSession,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let policy = workspace_access_policy(workspace_repository, run_id).await?;
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
    let Some(path) = required_trimmed_string_arg(args, "path") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "path is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(old_string) = required_raw_string_arg(args, "old_string") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "old_string is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(new_string) = required_raw_string_arg(args, "new_string") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "new_string is required"),
            AgentToolEffect::None,
        ));
    };
    let replace_all = match optional_bool_arg(args, "replace_all") {
        Ok(replace_all) => replace_all.unwrap_or(false),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    if old_string.is_empty() {
        return Ok((
            tool_error(
                call,
                "workspace.patch_empty_old_string",
                "old_string cannot be empty",
            ),
            AgentToolEffect::None,
        ));
    }
    if old_string == new_string {
        return Ok((
            tool_error(
                call,
                "workspace.patch_no_change",
                "old_string and new_string are identical",
            ),
            AgentToolEffect::None,
        ));
    }

    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_writable_workspace_path(call, &policy, &path) {
        return Ok((result, AgentToolEffect::None));
    }
    let path_key = path.as_str().to_string();
    let Some(read_state) = session.read_state(&path_key) else {
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_read",
                "file must be read with workspace_read_file before applying a patch",
            ),
            AgentToolEffect::None,
        ));
    };
    if !read_state.full_read {
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_full_read",
                "file must be fully read before applying a patch",
            ),
            AgentToolEffect::None,
        ));
    }

    let file = match workspace_repository.read_text(run_id, &path).await {
        Ok(file) => file,
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "workspace.file_not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    if file.sha256 != read_state.sha256 {
        return Ok((
            tool_error(
                call,
                "workspace.patch_stale_file",
                &format!(
                    "file changed since last full read: previous sha256 {}, current sha256 {}",
                    read_state.sha256, file.sha256
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let matches = file.text.matches(old_string).count();
    if matches == 0 {
        return Ok((
            tool_error(
                call,
                "workspace.patch_old_string_not_found",
                "old_string was not found in the file",
            ),
            AgentToolEffect::None,
        ));
    }
    if matches > 1 && !replace_all {
        return Ok((
            tool_error(
                call,
                "workspace.patch_old_string_not_unique",
                &format!(
                    "old_string matched {matches} times; provide more context or set replace_all=true"
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let updated = if replace_all {
        file.text.replace(old_string, new_string)
    } else {
        file.text.replacen(old_string, new_string, 1)
    };
    let old_sha256 = file.sha256.clone();
    let file = workspace_repository
        .write_text(run_id, &path, &updated)
        .await?;
    session.remember_file(&file, true);

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!(
            "Patched {} with {} replacement(s).",
            file.path.as_str(),
            matches
        ),
        structured: json!({
            "path": file.path.as_str(),
            "bytes": file.bytes,
            "oldSha256": old_sha256,
            "sha256": file.sha256.as_str(),
            "replacements": matches,
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![file.path.as_str().to_string()],
    };

    Ok((
        result,
        AgentToolEffect::WorkspaceFilePatched {
            file,
            replacements: matches,
            old_sha256,
        },
    ))
}
