use serde_json::json;

use super::args::{
    ensure_visible_workspace_path, object_args, optional_usize_arg, parse_workspace_path,
    required_trimmed_string_arg, tool_error,
};
use super::policy::workspace_access_policy;
use super::render::{format_lines_with_numbers, split_lines_for_display};
use super::{MAX_READ_BYTES, MAX_READ_LINES};
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;

pub(in crate::application::services::agent_tools) async fn read_file(
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
    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_visible_workspace_path(call, &policy, &path) {
        return Ok((result, AgentToolEffect::None));
    }

    let start_line = match optional_usize_arg(args, "start_line") {
        Ok(start_line) => start_line.unwrap_or(1),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if start_line == 0 {
        return Ok((
            tool_error(
                call,
                "workspace.invalid_line_range",
                "start_line must be >= 1",
            ),
            AgentToolEffect::None,
        ));
    }
    let line_count = match optional_usize_arg(args, "line_count") {
        Ok(line_count) => line_count,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if line_count == Some(0) {
        return Ok((
            tool_error(
                call,
                "workspace.invalid_line_range",
                "line_count must be >= 1",
            ),
            AgentToolEffect::None,
        ));
    }
    if line_count.is_some_and(|value| value > MAX_READ_LINES) {
        return Ok((
            tool_error(
                call,
                "workspace.read_line_count_too_large",
                &format!("line_count must be <= {MAX_READ_LINES}"),
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

    let lines = split_lines_for_display(&file.text);
    let total_lines = lines.len();
    if start_line > total_lines.max(1) {
        return Ok((
            tool_error(
                call,
                "workspace.invalid_line_range",
                &format!("start_line {start_line} is beyond total lines {total_lines}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let full_read_requested = start_line == 1 && line_count.is_none();
    if full_read_requested && (file.bytes > MAX_READ_BYTES || total_lines > MAX_READ_LINES) {
        return Ok((
            tool_error(
                call,
                "workspace.read_too_large",
                &format!(
                    "file is too large for a full read: {} bytes, {} lines. Use start_line and line_count.",
                    file.bytes, total_lines
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let end_line = match line_count {
        Some(count) => (start_line + count - 1).min(total_lines),
        None => total_lines,
    };
    if end_line.saturating_sub(start_line) + 1 > MAX_READ_LINES {
        return Ok((
            tool_error(
                call,
                "workspace.read_line_count_too_large",
                &format!("read range must be <= {MAX_READ_LINES} lines"),
            ),
            AgentToolEffect::None,
        ));
    }

    let selected = if total_lines == 0 {
        Vec::new()
    } else {
        lines[start_line - 1..end_line].to_vec()
    };
    let full_read = start_line == 1 && (total_lines == 0 || end_line == total_lines);
    session.remember_file(&file, full_read);

    let content = format!(
        "{} lines {}-{} of {}, sha256 {}\n{}",
        file.path.as_str(),
        if total_lines == 0 { 0 } else { start_line },
        if total_lines == 0 { 0 } else { end_line },
        total_lines,
        file.sha256,
        format_lines_with_numbers(&selected, start_line),
    );

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "path": file.path.as_str(),
                "bytes": file.bytes,
                "sha256": file.sha256.as_str(),
                "totalLines": total_lines,
                "startLine": if total_lines == 0 { 0 } else { start_line },
                "endLine": if total_lines == 0 { 0 } else { end_line },
                "fullRead": full_read,
            }),
            is_error: false,
            error_code: None,
            resource_refs: vec![file.path.as_str().to_string()],
        },
        AgentToolEffect::None,
    ))
}
