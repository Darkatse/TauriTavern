use serde::Serialize;

use super::args::{
    ensure_visible_workspace_path, object_args, parse_workspace_path, required_trimmed_string_arg,
    tool_error,
};
use super::policy::workspace_access_policy;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentChatCommitMode, AgentToolCall, AgentToolResult};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

use super::super::dispatcher::AgentToolEffect;
use super::super::structured::structured_value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCommitStructured<'a> {
    path: &'a str,
    mode: AgentChatCommitMode,
    reason: Option<&'a str>,
}

pub(in crate::application::services::agent_tools) async fn commit(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
    profile: &ResolvedAgentProfile,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let policy = workspace_access_policy(workspace_repository, run_id).await?;
    let args = object_args(call);
    let path = args
        .and_then(|args| required_trimmed_string_arg(args, "path"))
        .unwrap_or(profile.output.message_body_path.as_str());
    let path = match parse_workspace_path(call, path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };
    if let Err(result) = ensure_visible_workspace_path(call, &policy, &path) {
        return Ok((result, AgentToolEffect::None));
    }

    let mode = match args.and_then(|args| required_trimmed_string_arg(args, "mode")) {
        Some("append") => AgentChatCommitMode::Append,
        Some("replace") | None => AgentChatCommitMode::Replace,
        Some(other) => {
            return Ok((
                tool_error(
                    call,
                    "workspace.commit_mode_invalid",
                    &format!("mode must be `replace` or `append`, got `{other}`"),
                ),
                AgentToolEffect::None,
            ));
        }
    };
    let reason = args
        .and_then(|args| required_trimmed_string_arg(args, "reason"))
        .map(str::to_string);

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content: format!(
                "Requested chat commit of {} with mode {:?}.",
                path.as_str(),
                mode
            ),
            structured: structured_value(WorkspaceCommitStructured {
                path: path.as_str(),
                mode,
                reason: reason.as_deref(),
            }),
            is_error: false,
            error_code: None,
            resource_refs: vec![path.as_str().to_string()],
        },
        AgentToolEffect::ChatCommitRequested { path, mode, reason },
    ))
}
