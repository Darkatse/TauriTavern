use serde::Serialize;

use super::args::{
    classify_workspace_io_error, ensure_only_args, ensure_visible_workspace_path, object_args,
    optional_usize_arg, parse_workspace_path, required_trimmed_string_arg, tool_error,
};
use super::policy::workspace_access_policy;
use super::{MAX_READ_BYTES, MAX_READ_CHARS, MAX_READ_LINES};
use crate::errors::ApplicationError;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;
use tt_domain::text_lines::TextLineSelection;
use tt_domain::text_metrics::TextMetrics;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::super::structured::{TextLineRangePayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceReadFileStructured<'a> {
    path: &'a str,
    sha256: &'a str,
    #[serde(flatten)]
    range: TextLineRangePayload,
    full_read: bool,
}

pub(in crate::services::agent_tools) async fn read_file(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &ToolInvocation,
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
    if let Err(message) = ensure_only_args(args, &["path", "start_line", "line_count"]) {
        return Ok((
            tool_error(call, "tool.invalid_arguments", &message),
            AgentToolEffect::None,
        ));
    }
    let Some(path) = required_trimmed_string_arg(args, "path") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "path is required"),
            AgentToolEffect::None,
        ));
    };
    let path = match parse_workspace_path(path) {
        Ok(path) => path,
        Err(error) => return Ok((error.into_tool_result(call), AgentToolEffect::None)),
    };
    if let Err(error) = ensure_visible_workspace_path(&policy, &path) {
        return Ok((error.into_tool_result(call), AgentToolEffect::None));
    }

    let start_line = match optional_usize_arg(args, "start_line") {
        Ok(value) => value.unwrap_or(1),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    let line_count = match optional_usize_arg(args, "line_count") {
        Ok(value) => value,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    let file = match workspace_repository.read_text(run_id, &path).await {
        Ok(file) => file,
        Err(error) => match classify_workspace_io_error(call, error) {
            Ok(result) => return Ok((result, AgentToolEffect::None)),
            Err(error) => return Err(error.into()),
        },
    };
    let total_metrics = TextMetrics::from_text(&file.text);
    let total_lines = if file.text.is_empty() {
        0
    } else {
        file.text.split('\n').count()
    };
    let full_read_requested = start_line == 1 && line_count.is_none();
    let full_read_fits = file.bytes <= MAX_READ_BYTES
        && total_lines <= MAX_READ_LINES
        && total_metrics.chars <= MAX_READ_CHARS;
    let max_chars = if full_read_requested && full_read_fits {
        usize::MAX
    } else {
        MAX_READ_CHARS
    };
    let selection = match TextLineSelection::select(
        &file.text,
        start_line,
        line_count,
        MAX_READ_LINES,
        max_chars,
    ) {
        Ok(selection) => selection,
        Err(error) => {
            return Ok((
                tool_error(call, "workspace.invalid_line_range", &error.to_string()),
                AgentToolEffect::None,
            ));
        }
    };
    let selected_metrics = TextMetrics::from_text(&selection.content);
    let full_read = !selection.truncated();
    session.remember_file_read(&file, full_read, &selection.content);

    let mut content = format!(
        "{} lines {}-{} of {}, chars {} of {}, words {} of {}, sha256 {}{}",
        file.path.as_str(),
        selection.start_line,
        selection.end_line,
        selection.total_lines,
        selected_metrics.chars,
        total_metrics.chars,
        selected_metrics.words,
        total_metrics.words,
        file.sha256,
        if selection.truncated() {
            " (preview)"
        } else {
            ""
        },
    );
    let numbered = selection.numbered_content();
    if !numbered.is_empty() {
        content.push('\n');
        content.push_str(&numbered);
    }
    if let Some(next_start_line) = selection.next_start_line() {
        content.push_str(&format!(
            "\n\nPreview ended before the file. Continue with start_line={next_start_line} and line_count={}.",
            selection.returned_line_count()
        ));
    }
    if selection.line_truncated {
        content.push_str(&format!(
            "\n\nLine {} is longer than this reader can return safely, so only its beginning is shown. Search this file for specific text if you need a later section.",
            selection.start_line
        ));
    }

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: structured_value(WorkspaceReadFileStructured {
                path: file.path.as_str(),
                sha256: file.sha256.as_str(),
                range: TextLineRangePayload::new(
                    selected_metrics,
                    total_metrics,
                    selection.total_lines,
                    selection.start_line,
                    selection.end_line,
                    selection.line_truncated,
                ),
                full_read,
            }),
            is_error: false,
            error_code: None,
            resource_refs: vec![file.path.as_str().to_string()],
        },
        AgentToolEffect::None,
    ))
}
