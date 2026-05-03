use serde_json::json;

use super::super::common::{
    object_args, optional_usize_arg, required_trimmed_string_arg, tool_error,
};
use super::super::dispatcher::AgentToolEffect;
use crate::application::errors::ApplicationError;
use crate::application::services::skill_service::SkillService;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

pub(in crate::application::services::agent_tools) async fn read(
    skill_service: &SkillService,
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
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
    let Some(name) = required_trimmed_string_arg(args, "name") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "name is required"),
            AgentToolEffect::None,
        ));
    };
    let path = args
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("SKILL.md");
    let max_chars = match optional_usize_arg(args, "max_chars") {
        Ok(value) => value,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    let read = match skill_service.read_skill_file(name, path, max_chars).await {
        Ok(read) => read,
        Err(ApplicationError::ValidationError(message)) => {
            return Ok((
                tool_error(call, "skill.invalid_request", &message),
                AgentToolEffect::None,
            ));
        }
        Err(ApplicationError::NotFound(message)) => {
            return Ok((
                tool_error(call, "skill.not_found", &message),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error),
    };

    let content = format!(
        "{} chars from {}, sha256 {}{}\n{}",
        read.chars,
        read.resource_ref,
        read.sha256,
        if read.truncated { " (truncated)" } else { "" },
        read.content
    );
    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: json!({
                "name": read.name,
                "path": read.path,
                "bytes": read.bytes,
                "sha256": read.sha256,
                "chars": read.chars,
                "truncated": read.truncated,
                "resourceRef": read.resource_ref,
            }),
            is_error: false,
            error_code: None,
            resource_refs: vec![read.resource_ref],
        },
        AgentToolEffect::None,
    ))
}
