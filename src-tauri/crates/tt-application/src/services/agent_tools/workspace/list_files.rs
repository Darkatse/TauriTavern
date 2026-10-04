use serde::Serialize;
use serde_json::{Map, Value};

use super::args::{
    ensure_visible_workspace_path, optional_list_path_arg, optional_usize_arg, tool_error,
};
use super::render::render_file_list;
use super::{
    DEFAULT_LIST_DEPTH, MAX_INDEX_EXISTING_FILES, MAX_INDEX_STATE_ENTRIES, MAX_LIST_DEPTH,
    MAX_LIST_ENTRIES,
};
use crate::errors::ApplicationError;
use crate::services::agent_profile_service::workspace_roots_from_profile;
use crate::services::agent_workspace_scope::ScopedWorkspaceFs;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentRunTarget, AgentToolResult, WorkspacePath, WorkspaceRootLifecycle,
};
use tt_domain::models::tool::ToolInvocation;
use tt_ports::workspace_fs::{WorkspaceEntryKind, WorkspaceFs};

use super::super::dispatcher::AgentToolEffect;
use super::super::structured::structured_value;

const PERSIST_ROOT: &str = "persist";

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

/// The workspace index at the top of the invocation's agent system prompt: the chat
/// files, which roots are work files, files earlier stages of this Run wrote, and the
/// first level of persistent state, so the model does not spend a round listing the
/// workspace. It sits at the head of the cached prompt prefix, so it lists only what holds
/// while that prefix is reused: nothing that changes every floor (floor numbers, times),
/// and no files in roots that outlive the Run. A Chat Run's index changes only when a
/// top-level `persist/` entry is added or removed; a Session's does not change. Skill
/// packages and tool results have their own entry points (the Skill catalog and the
/// result that names them).
pub(crate) async fn render_workspace_index(
    workspace: &ScopedWorkspaceFs,
    profile: &ResolvedAgentProfile,
    target: &AgentRunTarget,
) -> Result<String, ApplicationError> {
    let roots = &profile.workspace;
    let writable = |root: &str| roots.writable_roots.iter().any(|writable| writable == root);
    let mut lines = vec!["# Workspace".to_string()];
    if workspace.has_chat() {
        lines.push(
            "Chat (read-only): chat.json, floors/NNNNNN/{message.md (raw),meta.json}".to_string(),
        );
    }

    let (work, read_only): (Vec<&str>, Vec<&str>) = roots
        .visible_roots
        .iter()
        .map(String::as_str)
        .filter(|root| *root != PERSIST_ROOT)
        .partition(|root| writable(root));
    let dirs = |roots: &[&str]| {
        roots
            .iter()
            .map(|root| format!("{root}/"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    // Roots that end with the Run are marked `this run` and list their files; a Session
    // keeps its roots across runs.
    let specs = workspace_roots_from_profile(profile, target);
    let ends_with_run = |root: &str| {
        specs
            .iter()
            .any(|spec| spec.path == root && spec.lifecycle == WorkspaceRootLifecycle::Run)
    };
    let this_run = work
        .iter()
        .chain(&read_only)
        .all(|root| ends_with_run(root));
    let note = |read_only: bool| match (read_only, this_run) {
        (false, false) => "",
        (false, true) => " (this run)",
        (true, false) => " (read-only)",
        (true, true) => " (read-only, this run)",
    };
    match (work.is_empty(), read_only.is_empty()) {
        (false, true) => lines.push(format!("Work: {}{}", dirs(&work), note(false))),
        (false, false) => lines.push(format!(
            "Work: {}{}; read-only: {}",
            dirs(&work),
            note(false),
            dirs(&read_only)
        )),
        (true, false) => lines.push(format!("Work: {}{}", dirs(&read_only), note(true))),
        (true, true) => {}
    }

    // A Chat Run starts with empty work roots, so this line stays out of its cached
    // prefix. Handoff targets and subagents start after earlier writes in the same Run
    // and should see the ones their own roots allow. Files in roots that outlive the Run
    // change between runs, so they are not listed.
    let workspace_files: &dyn WorkspaceFs = workspace;
    let mut existing = Vec::new();
    let mut truncated = false;
    for root in work
        .iter()
        .chain(&read_only)
        .filter(|root| ends_with_run(root))
    {
        let list = match workspace_files
            .list_files(
                Some(&WorkspacePath::parse(root)?),
                MAX_LIST_DEPTH,
                MAX_LIST_ENTRIES,
            )
            .await
        {
            Ok(list) => list,
            // A visible root that was never created holds no files.
            Err(DomainError::NotFound(_)) => continue,
            Err(error) => return Err(error.into()),
        };
        truncated |= list.truncated;
        existing.extend(
            list.entries
                .into_iter()
                .filter(|entry| entry.kind == WorkspaceEntryKind::File)
                .map(|entry| entry.path.as_str().to_string()),
        );
    }
    if !existing.is_empty() {
        existing.sort_unstable();
        truncated |= existing.len() > MAX_INDEX_EXISTING_FILES;
        existing.truncate(MAX_INDEX_EXISTING_FILES);
        if truncated {
            existing.push("\u{2026}".to_string());
        }
        lines.push(format!("Existing: {}", existing.join(", ")));
    }

    if roots.visible_roots.iter().any(|root| root == PERSIST_ROOT) {
        let access = if writable(PERSIST_ROOT) {
            "writable"
        } else {
            "read-only"
        };
        let entries = match workspace_files
            .read_dir(Some(&WorkspacePath::parse(PERSIST_ROOT)?), usize::MAX)
            .await
        {
            Ok(entries) => entries,
            // A visible root that was never created holds no files.
            Err(DomainError::NotFound(_)) => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        if entries.is_empty() {
            lines.push(format!("State: persist/ (empty; {access}, kept per floor)"));
        } else {
            let mut names = entries
                .iter()
                .map(|entry| {
                    let name = entry
                        .path
                        .as_str()
                        .rsplit_once('/')
                        .map_or(entry.path.as_str(), |(_, name)| name);
                    match entry.metadata.kind {
                        WorkspaceEntryKind::Directory => format!("{name}/"),
                        WorkspaceEntryKind::File => name.to_string(),
                    }
                })
                .collect::<Vec<_>>();
            // Directory order is platform-dependent; the cached prefix must not be.
            names.sort_unstable();
            if names.len() > MAX_INDEX_STATE_ENTRIES {
                names.truncate(MAX_INDEX_STATE_ENTRIES);
                names.push("\u{2026}".to_string());
            }
            lines.push(format!(
                "State: persist/{{{}}} ({access}, kept per floor)",
                names.join(",")
            ));
        }
    }

    Ok(if lines.len() == 1 {
        String::new()
    } else {
        lines.join("\n")
    })
}
