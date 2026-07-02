use serde::Serialize;

use super::args::{
    classify_workspace_io_error, ensure_writable_workspace_path, object_args, optional_bool_arg,
    parse_workspace_path, required_raw_string_arg, required_trimmed_string_arg, tool_error,
};
use super::policy::workspace_access_policy;
use crate::errors::ApplicationError;
use tt_domain::errors::{DomainError, WorkspaceWriteConflictKind};
use tt_domain::models::agent::{AgentToolCall, AgentToolResult};
use tt_domain::text_metrics::TextMetrics;
use tt_ports::repositories::workspace_repository::{WorkspaceRepository, WorkspaceWriteGuard};

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::super::structured::{TextMetricsPayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceApplyPatchStructured<'a> {
    path: &'a str,
    #[serde(flatten)]
    metrics: TextMetricsPayload,
    old_sha256: &'a str,
    sha256: &'a str,
    replacements: usize,
}

pub(in crate::services::agent_tools) async fn apply_patch(
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
    let Some(read_state) = session.read_state(&path_key).cloned() else {
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_read",
                "file must be read with workspace_read_file before applying a patch",
            ),
            AgentToolEffect::None,
        ));
    };
    if read_state.patch_requires_full_read() {
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_full_read",
                "a previous patch attempt for this file failed. Fully read the file with workspace_read_file before applying another patch.",
            ),
            AgentToolEffect::None,
        ));
    }
    let patch_uses_partial_read = !read_state.full_read;
    if patch_uses_partial_read && replace_all {
        session.require_full_read_before_patch(&path_key);
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_full_read",
                "replace_all can modify text outside the range you read. Fully read the file with workspace_read_file before using replace_all.",
            ),
            AgentToolEffect::None,
        ));
    }
    if patch_uses_partial_read && !read_state.old_string_was_observed(old_string) {
        session.require_full_read_before_patch(&path_key);
        return Ok((
            tool_error(
                call,
                "workspace.patch_requires_full_read",
                "old_string was not in the text you have read for this file. Fully read the file with workspace_read_file before retrying the patch.",
            ),
            AgentToolEffect::None,
        ));
    }

    let file = match workspace_repository.read_text(run_id, &path).await {
        Ok(file) => file,
        Err(error) => match classify_workspace_io_error(call, error) {
            Ok(result) => return Ok((result, AgentToolEffect::None)),
            Err(error) => return Err(error.into()),
        },
    };
    if file.sha256 != read_state.sha256 {
        if patch_uses_partial_read {
            session.require_full_read_before_patch(&path_key);
        }
        return Ok((
            tool_error(
                call,
                "workspace.patch_stale_file",
                if patch_uses_partial_read {
                    "file changed since your last read. Fully read the file with workspace_read_file before patching it."
                } else {
                    "file changed since your last full read. Read the file again before patching it."
                },
            ),
            AgentToolEffect::None,
        ));
    }

    let matches = file.text.matches(old_string).count();
    if matches == 0 {
        if patch_uses_partial_read {
            session.require_full_read_before_patch(&path_key);
        }
        return Ok((
            tool_error(
                call,
                "workspace.patch_old_string_not_found",
                if patch_uses_partial_read {
                    "old_string was not found in the file. Fully read the file with workspace_read_file before retrying the patch."
                } else {
                    "old_string was not found in the file"
                },
            ),
            AgentToolEffect::None,
        ));
    }
    if matches > 1 && !replace_all {
        if patch_uses_partial_read {
            session.require_full_read_before_patch(&path_key);
        }
        return Ok((
            tool_error(
                call,
                "workspace.patch_old_string_not_unique",
                &patch_not_unique_message(matches, patch_uses_partial_read),
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
    let file = match workspace_repository
        .write_text_guarded(
            run_id,
            &path,
            &updated,
            WorkspaceWriteGuard::MustMatchSha256(old_sha256.clone()),
        )
        .await
    {
        Ok(file) => file,
        Err(DomainError::WorkspaceWriteConflict { kind, .. }) => {
            if patch_uses_partial_read {
                session.require_full_read_before_patch(&path_key);
            }
            return Ok((
                patch_conflict_error(call, kind, patch_uses_partial_read),
                AgentToolEffect::None,
            ));
        }
        Err(error) => match classify_workspace_io_error(call, error) {
            Ok(result) => return Ok((result, AgentToolEffect::None)),
            Err(error) => return Err(error.into()),
        },
    };
    if patch_uses_partial_read {
        session.remember_partial_patch(&file, old_string, new_string);
    } else {
        session.remember_file(&file, true);
    }
    let metrics = TextMetrics::from_text(&file.text);

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!(
            "Patched {} with {} replacement(s); file now has {} chars / {} words.",
            file.path.as_str(),
            matches,
            metrics.chars,
            metrics.words
        ),
        structured: structured_value(WorkspaceApplyPatchStructured {
            path: file.path.as_str(),
            metrics: metrics.into(),
            old_sha256: old_sha256.as_str(),
            sha256: file.sha256.as_str(),
            replacements: matches,
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

fn patch_not_unique_message(matches: usize, require_full_read: bool) -> String {
    if require_full_read {
        format!(
            "old_string matched {matches} times in the file. Fully read the file with workspace_read_file before retrying with more context, or use replace_all after a full read."
        )
    } else {
        format!("old_string matched {matches} times; provide more context or set replace_all=true")
    }
}

fn patch_conflict_error(
    call: &AgentToolCall,
    kind: WorkspaceWriteConflictKind,
    require_full_read: bool,
) -> AgentToolResult {
    let changed_message = if require_full_read {
        "file changed before the patch could be written. Fully read the file with workspace_read_file before patching it."
    } else {
        "file changed before the patch could be written. Read the file again before patching it."
    };
    match kind {
        WorkspaceWriteConflictKind::AlreadyExists { .. } => {
            tool_error(call, "workspace.patch_stale_file", changed_message)
        }
        WorkspaceWriteConflictKind::Stale {
            actual_sha256: Some(_),
            ..
        } => tool_error(call, "workspace.patch_stale_file", changed_message),
        WorkspaceWriteConflictKind::Stale {
            actual_sha256: None,
            ..
        } => tool_error(
            call,
            "workspace.patch_stale_file",
            "file changed before the patch could be written and is no longer present. Read the parent directory before patching again.",
        ),
    }
}
