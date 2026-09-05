use serde::Serialize;

use super::super::common::{
    ensure_only_args, object_args, optional_usize_arg, required_trimmed_string_arg, tool_error,
};
use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::list::skill_is_visible;
use crate::errors::ApplicationError;
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::skill::SkillReadRequest;
use tt_domain::models::tool::ToolInvocation;
use tt_domain::text_lines::format_lines_with_numbers;
use tt_domain::text_metrics::TextMetrics;

use super::super::structured::{TextLineRangePayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillReadStructured<'a> {
    name: &'a str,
    path: &'a str,
    sha256: &'a str,
    #[serde(flatten)]
    range: TextLineRangePayload,
    resource_ref: &'a str,
}

pub(in crate::services::agent_tools) async fn read(
    skill_service: &SkillService,
    call: &ToolInvocation,
    session: &mut AgentToolSession,
    profile: &ResolvedAgentProfile,
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
    if let Err(message) = ensure_only_args(args, &["name", "path", "start_line", "line_count"]) {
        return Ok((
            tool_error(call, "tool.invalid_arguments", &message),
            AgentToolEffect::None,
        ));
    }
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
    let start_line = match optional_usize_arg(args, "start_line") {
        Ok(value) => value,
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
    if !skill_is_visible(&profile.skills, name) {
        return Ok((
            tool_error(
                call,
                "skill.policy_denied",
                &format!("Skill `{name}` is not available under the current policy."),
            ),
            AgentToolEffect::None,
        ));
    }
    let Some(scope) = session.effective_skill_scope(name) else {
        return Ok((
            tool_error(
                call,
                "skill.not_visible",
                &format!("Skill `{name}` is not available in the current Skill set."),
            ),
            AgentToolEffect::None,
        ));
    };
    let remaining = profile
        .skills
        .max_read_chars_per_run
        .saturating_sub(session.skill_read_chars());
    if remaining == 0 {
        return Ok((
            tool_error(
                call,
                "skill.read_budget_exhausted",
                "Skill read budget is exhausted for this run.",
            ),
            AgentToolEffect::None,
        ));
    }
    let max_output_chars = profile.skills.max_read_chars_per_call.min(remaining);

    let read = match skill_service
        .read_skill_file(SkillReadRequest {
            frozen_macros: Some(session.frozen_macros.clone()),
            scope,
            name: name.to_string(),
            path: path.to_string(),
            start_line,
            line_count,
            max_output_chars,
        })
        .await
    {
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
    session.remember_skill_read_chars(read.chars);

    let mut content = format!(
        "{} chars / {} words from {}, lines {}-{} of {}{}",
        read.chars,
        read.words,
        read.resource_ref.as_str(),
        read.start_line,
        read.end_line,
        read.total_lines,
        if read.truncated { " (truncated)" } else { "" },
    );
    let numbered = format_lines_with_numbers(&read.content, read.start_line, read.end_line);
    if !numbered.is_empty() {
        content.push('\n');
        content.push_str(&numbered);
    }
    if let Some(next_start_line) = read.next_start_line {
        content.push_str(&format!(
            "\n\nPreview ended before the file. Continue with start_line={next_start_line} and line_count={}.",
            read.end_line - read.start_line + 1
        ));
    }
    if read.line_truncated {
        content.push_str(&format!(
            "\n\nLine {} exceeds the Skill read budget and was truncated.",
            read.start_line
        ));
    }
    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: structured_value(SkillReadStructured {
                name: read.name.as_str(),
                path: read.path.as_str(),
                sha256: read.sha256.as_str(),
                range: TextLineRangePayload::new(
                    TextMetrics {
                        chars: read.chars,
                        words: read.words,
                    },
                    TextMetrics {
                        chars: read.total_chars,
                        words: read.total_words,
                    },
                    read.total_lines,
                    read.start_line,
                    read.end_line,
                    read.line_truncated,
                ),
                resource_ref: read.resource_ref.as_str(),
            }),
            is_error: false,
            error_code: None,
            resource_refs: vec![read.resource_ref],
        },
        AgentToolEffect::None,
    ))
}
