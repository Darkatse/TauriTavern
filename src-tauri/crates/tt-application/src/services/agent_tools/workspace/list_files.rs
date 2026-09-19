use serde::Serialize;
use serde_json::{Map, Value};

use super::args::{
    ensure_visible_workspace_path, optional_list_path_arg, optional_usize_arg, tool_error,
};
use super::render::render_file_list;
use super::{DEFAULT_LIST_DEPTH, MAX_LIST_DEPTH, MAX_LIST_ENTRIES};
use crate::errors::ApplicationError;
use crate::services::agent_workspace_scope::ScopedWorkspaceFs;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;
use tt_ports::workspace_fs::{WorkspaceEntryKind, WorkspaceFs};

use super::super::dispatcher::AgentToolEffect;
use super::super::structured::structured_value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceListFilesStructured<'a> {
    entries: Vec<WorkspaceListEntryStructured<'a>>,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceListEntryStructured<'a> {
    path: &'a str,
    kind: &'static str,
}

pub(in crate::services::agent_tools) async fn list_files(
    workspace: &ScopedWorkspaceFs,
    call: &ToolInvocation,
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let policy = &workspace.policy;
    let workspace_files: &dyn WorkspaceFs = workspace;
    let path = match optional_list_path_arg(args, "path") {
        Ok(path) => path,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if let Some(path) = &path
        && let Err(error) = ensure_visible_workspace_path(policy, path)
    {
        return Ok((error.into_tool_result(call), AgentToolEffect::None));
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

    let list = match workspace_files
        .list_files(path.as_ref(), depth, MAX_LIST_ENTRIES)
        .await
    {
        Ok(list) => list,
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
        .map(|entry| WorkspaceListEntryStructured {
            path: entry.path.as_str(),
            kind: match entry.kind {
                WorkspaceEntryKind::File => "file",
                WorkspaceEntryKind::Directory => "directory",
            },
        })
        .collect::<Vec<_>>();
    let content = render_file_list(&list);

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: structured_value(WorkspaceListFilesStructured {
                entries,
                truncated: list.truncated,
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
