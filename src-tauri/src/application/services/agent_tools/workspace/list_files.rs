use serde_json::json;

use super::args::{
    ensure_visible_workspace_path, object_args, optional_list_path_arg, optional_usize_arg,
    tool_error,
};
use super::policy::workspace_access_policy;
use super::render::{filter_visible_entries, render_file_list};
use super::{DEFAULT_LIST_DEPTH, MAX_LIST_DEPTH, MAX_LIST_ENTRIES};
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};
use crate::domain::repositories::workspace_repository::{WorkspaceEntryKind, WorkspaceRepository};

use super::super::dispatcher::AgentToolEffect;

pub(in crate::application::services::agent_tools) async fn list_files(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
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
    let path = match optional_list_path_arg(args, "path") {
        Ok(path) => path,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if let Some(path) = &path {
        if let Err(result) = ensure_visible_workspace_path(call, &policy, path) {
            return Ok((result, AgentToolEffect::None));
        }
    }
    let depth = match optional_usize_arg(args, "depth") {
        Ok(depth) => depth.unwrap_or(DEFAULT_LIST_DEPTH),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if depth > MAX_LIST_DEPTH {
        return Ok((
            tool_error(
                call,
                "workspace.list_depth_too_large",
                &format!("depth must be <= {MAX_LIST_DEPTH}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let list = match workspace_repository
        .list_files(run_id, path.as_ref(), depth, MAX_LIST_ENTRIES)
        .await
    {
        Ok(list) => filter_visible_entries(list, &policy),
        Err(DomainError::NotFound(message)) => {
            return Ok((
                tool_error(call, "workspace.path_not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error.into()),
    };

    let entries = list
        .entries
        .iter()
        .map(|entry| {
            json!({
                "path": entry.path.as_str(),
                "kind": match entry.kind {
                    WorkspaceEntryKind::File => "file",
                    WorkspaceEntryKind::Directory => "directory",
                },
                "bytes": entry.bytes,
            })
        })
        .collect::<Vec<_>>();
    let content = render_file_list(&list);

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "entries": entries,
                "truncated": list.truncated,
            }),
            is_error: false,
            error_code: None,
            resource_refs: list
                .entries
                .iter()
                .map(|entry| entry.path.as_str().to_string())
                .collect(),
        },
        AgentToolEffect::None,
    ))
}
