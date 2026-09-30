use serde::Serialize;
use serde_json::{Map, Value};

use super::args::{
    ensure_visible_workspace_path, optional_list_path_arg, optional_usize_arg, tool_error,
};
use super::render::render_file_list;
use super::{DEFAULT_LIST_DEPTH, MAX_INVENTORY_FILES, MAX_LIST_DEPTH, MAX_LIST_ENTRIES};
use crate::errors::ApplicationError;
use crate::services::agent_workspace_scope::{
    AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT, ScopedWorkspaceFs,
};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentToolResult, WorkspacePath};
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

/// Files visible to an invocation when it starts, rendered once into its prompt so the
/// model does not spend a round listing the workspace. Skill packages and tool results
/// have their own entry points (the Skill catalog and the result that names them).
pub(crate) async fn render_workspace_inventory(
    workspace: &ScopedWorkspaceFs,
) -> Result<String, ApplicationError> {
    let workspace_files: &dyn WorkspaceFs = workspace;
    let mut files = Vec::new();
    let mut truncated = false;
    let mut deeper = false;
    for root in workspace
        .policy
        .visible_roots
        .iter()
        .filter(|root| ![AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT].contains(&root.as_str()))
    {
        let root = WorkspacePath::parse(root)?;
        let list = match workspace_files
            .list_files(Some(&root), MAX_LIST_DEPTH, MAX_LIST_ENTRIES)
            .await
        {
            Ok(list) => list,
            // A visible root that was never created holds no files.
            Err(DomainError::NotFound(_)) => continue,
            Err(error) => return Err(error.into()),
        };
        truncated |= list.truncated;
        // Directories at this depth are listed but not expanded; their contents are
        // unknown here, so the inventory must not claim to be complete.
        let unexpanded_segments = path_segments(&root) + MAX_LIST_DEPTH + 1;
        deeper |= list.entries.iter().any(|entry| {
            entry.kind == WorkspaceEntryKind::Directory
                && path_segments(&entry.path) >= unexpanded_segments
        });
        files.extend(
            list.entries
                .into_iter()
                .filter(|entry| entry.kind == WorkspaceEntryKind::File)
                .map(|entry| entry.path),
        );
    }
    if files.is_empty() && !truncated && !deeper {
        let persist_visible = workspace
            .policy
            .visible_roots
            .iter()
            .any(|root| root == "persist");
        return Ok(if persist_visible {
            "Workspace files at start: none; persist/ is empty.".to_string()
        } else {
            "Workspace files at start: none.".to_string()
        });
    }
    files.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let mut lines = vec!["Workspace files at start:".to_string()];
    lines.extend(
        files
            .iter()
            .take(MAX_INVENTORY_FILES)
            .map(|path| format!("- {}", path.as_str())),
    );
    let hidden = files.len().saturating_sub(MAX_INVENTORY_FILES);
    if truncated {
        lines.push("- ... more files not shown".to_string());
    } else if hidden > 0 {
        lines.push(format!("- ... {hidden} more files"));
    }
    if deeper && !truncated {
        lines.push("- ... files in deeper directories not listed".to_string());
    }
    Ok(lines.join("\n"))
}

fn path_segments(path: &WorkspacePath) -> usize {
    path.as_str().split('/').count()
}
