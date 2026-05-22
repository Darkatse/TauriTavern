use serde::Serialize;

use super::args::{
    classify_workspace_io_error, ensure_writable_workspace_path, object_args, parse_workspace_path,
    required_raw_string_arg, required_trimmed_string_arg, tool_error,
};
use super::policy::workspace_access_policy;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;
use crate::domain::text_metrics::TextMetrics;

use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::super::structured::{TextMetricsPayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceWriteFileStructured<'a> {
    path: &'a str,
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

    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_writable_workspace_path(call, &policy, &path) {
        return Ok((result, AgentToolEffect::None));
    }
    let file = match workspace_repository
        .write_text(run_id, &path, content)
        .await
    {
        Ok(file) => file,
        Err(error) => match classify_workspace_io_error(call, error) {
            Ok(result) => return Ok((result, AgentToolEffect::None)),
            Err(error) => return Err(error.into()),
        },
    };
    session.remember_file(&file, true);
    let metrics = TextMetrics::from_text(&file.text);

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!(
            "Wrote {} chars / {} words to {}.",
            metrics.chars,
            metrics.words,
            file.path.as_str()
        ),
        structured: structured_value(WorkspaceWriteFileStructured {
            path: file.path.as_str(),
            metrics: metrics.into(),
            sha256: file.sha256.as_str(),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![file.path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::WorkspaceFileWritten { file }))
}
