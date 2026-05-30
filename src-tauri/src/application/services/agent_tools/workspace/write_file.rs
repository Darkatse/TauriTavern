use serde::Serialize;
use serde_json::{Map, Value};

use super::args::{
    classify_workspace_io_error, ensure_writable_workspace_path, object_args, parse_workspace_path,
    required_raw_string_arg, required_trimmed_string_arg, tool_error,
};
use super::policy::workspace_access_policy;
use crate::application::errors::ApplicationError;
use crate::domain::errors::{DomainError, WorkspaceWriteConflictKind};
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspaceFileWriteMode};
use crate::domain::repositories::workspace_repository::WorkspaceAppendResult;
use crate::domain::repositories::workspace_repository::{WorkspaceRepository, WorkspaceWriteGuard};
use crate::domain::text_metrics::TextMetrics;

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::super::structured::{TextMetricsPayload, structured_value};

const WRITE_MODE_INVALID_MESSAGE: &str = "mode must be `replace` or `append`";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceWriteFileStructured<'a> {
    path: &'a str,
    mode: WorkspaceFileWriteMode,
    #[serde(flatten)]
    metrics: TextMetricsPayload,
    sha256: &'a str,
}

pub(in crate::application::services::agent_tools) async fn write_file(
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
    let Some(content) = required_raw_string_arg(args, "content") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "content is required"),
            AgentToolEffect::None,
        ));
    };
    let mode = match parse_write_mode(args) {
        Ok(mode) => mode,
        Err(message) => {
            return Ok((
                tool_error(call, "workspace.write_mode_invalid", message),
                AgentToolEffect::None,
            ));
        }
    };

    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_writable_workspace_path(call, &policy, &path) {
        return Ok((result, AgentToolEffect::None));
    }
    let (file, file_is_fully_known) = match mode {
        WorkspaceFileWriteMode::Replace => {
            let write_guard = match workspace_repository.read_text(run_id, &path).await {
                Ok(current) => {
                    let Some(read_state) = session.read_state(path.as_str()) else {
                        return Ok((
                            tool_error(
                                call,
                                "workspace.write_requires_read",
                                "file already exists; read it with workspace_read_file before rewriting it",
                            ),
                            AgentToolEffect::None,
                        ));
                    };
                    if current.sha256 != read_state.sha256 {
                        return Ok((
                            tool_error(
                                call,
                                "workspace.write_stale_file",
                                "file changed since you last read or wrote it. Read the file again before rewriting it.",
                            ),
                            AgentToolEffect::None,
                        ));
                    }
                    WorkspaceWriteGuard::MustMatchSha256(read_state.sha256.clone())
                }
                Err(DomainError::NotFound(_)) => WorkspaceWriteGuard::MustNotExist,
                Err(error) => match classify_workspace_io_error(call, error) {
                    Ok(result) => return Ok((result, AgentToolEffect::None)),
                    Err(error) => return Err(error.into()),
                },
            };
            match workspace_repository
                .write_text_guarded(run_id, &path, content, write_guard)
                .await
            {
                Ok(file) => (file, true),
                Err(DomainError::WorkspaceWriteConflict { kind, .. }) => {
                    return Ok((write_conflict_error(call, kind), AgentToolEffect::None));
                }
                Err(error) => match classify_workspace_io_error(call, error) {
                    Ok(result) => return Ok((result, AgentToolEffect::None)),
                    Err(error) => return Err(error.into()),
                },
            }
        }
        WorkspaceFileWriteMode::Append => {
            let result = match workspace_repository
                .append_text(run_id, &path, content)
                .await
            {
                Ok(result) => result,
                Err(error) => match classify_workspace_io_error(call, error) {
                    Ok(result) => return Ok((result, AgentToolEffect::None)),
                    Err(error) => return Err(error.into()),
                },
            };
            let file_is_fully_known = appended_file_is_fully_known(session, path.as_str(), &result);
            (result.file, file_is_fully_known)
        }
    };
    if file_is_fully_known {
        session.remember_file(&file, true);
    }
    let metrics = TextMetrics::from_text(&file.text);

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!(
            "{} {} chars / {} words to {}.",
            write_mode_past_tense(mode),
            metrics.chars,
            metrics.words,
            file.path.as_str()
        ),
        structured: structured_value(WorkspaceWriteFileStructured {
            path: file.path.as_str(),
            mode,
            metrics: metrics.into(),
            sha256: file.sha256.as_str(),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![file.path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::WorkspaceFileWritten { file, mode }))
}

fn parse_write_mode(args: &Map<String, Value>) -> Result<WorkspaceFileWriteMode, &'static str> {
    let Some(value) = args.get("mode") else {
        return Ok(WorkspaceFileWriteMode::Replace);
    };
    let Some(raw) = value.as_str() else {
        return Err(WRITE_MODE_INVALID_MESSAGE);
    };
    match raw.trim() {
        "replace" => Ok(WorkspaceFileWriteMode::Replace),
        "append" => Ok(WorkspaceFileWriteMode::Append),
        _ => Err(WRITE_MODE_INVALID_MESSAGE),
    }
}

fn appended_file_is_fully_known(
    session: &AgentToolSession,
    path: &str,
    result: &WorkspaceAppendResult,
) -> bool {
    match result.previous_sha256.as_deref() {
        None => true,
        Some(previous_sha256) => session
            .read_state(path)
            .is_some_and(|state| state.full_read && state.sha256 == previous_sha256),
    }
}

fn write_mode_past_tense(mode: WorkspaceFileWriteMode) -> &'static str {
    match mode {
        WorkspaceFileWriteMode::Replace => "Wrote",
        WorkspaceFileWriteMode::Append => "Appended",
    }
}

fn write_conflict_error(call: &AgentToolCall, kind: WorkspaceWriteConflictKind) -> AgentToolResult {
    match kind {
        WorkspaceWriteConflictKind::AlreadyExists { .. } => tool_error(
            call,
            "workspace.write_requires_read",
            "file already exists; read it with workspace_read_file before rewriting it",
        ),
        WorkspaceWriteConflictKind::Stale {
            actual_sha256: Some(_),
            ..
        } => tool_error(
            call,
            "workspace.write_stale_file",
            "file changed since you last read or wrote it. Read the file again before rewriting it.",
        ),
        WorkspaceWriteConflictKind::Stale {
            actual_sha256: None,
            ..
        } => tool_error(
            call,
            "workspace.write_stale_file",
            "file changed since you last read or wrote it and is no longer present. Read the parent directory before writing again.",
        ),
    }
}
