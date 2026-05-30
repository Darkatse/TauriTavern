use serde::Serialize;

use super::super::common::{
    object_args, optional_usize_arg, required_trimmed_string_arg, tool_error,
};
use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::list::skill_is_visible;
use crate::application::errors::ApplicationError;
use crate::application::services::skill_service::SkillService;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};
use crate::domain::models::skill::SkillReadRequest;
use crate::domain::text_metrics::TextMetrics;

use super::super::structured::{TextRangeMetricsPayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillReadStructured<'a> {
    name: &'a str,
    path: &'a str,
    sha256: &'a str,
    #[serde(flatten)]
    range: TextRangeMetricsPayload,
    total_lines: usize,
    start_line: usize,
    end_line: usize,
    resource_ref: &'a str,
}

pub(in crate::application::services::agent_tools) async fn read(
    skill_service: &SkillService,
    call: &AgentToolCall,
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
    let start_char = match optional_usize_arg(args, "start_char") {
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
    let effective_max_chars = match max_chars {
        Some(requested) if requested > profile.skills.max_read_chars_per_call => {
            return Ok((
                tool_error(
                    call,
                    "skill.read_budget_exceeded",
                    &format!(
                        "max_chars exceeds the current per-call Skill read budget of {}.",
                        profile.skills.max_read_chars_per_call
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        Some(requested) if requested > remaining => {
            return Ok((
                tool_error(
                    call,
                    "skill.read_budget_exhausted",
                    &format!("max_chars exceeds remaining run skill read budget of {remaining}."),
                ),
                AgentToolEffect::None,
            ));
        }
        Some(requested) => requested,
        None => profile.skills.max_read_chars_per_call.min(remaining),
    };

    let read = match skill_service
        .read_skill_file(SkillReadRequest {
            scope,
            name: name.to_string(),
            path: path.to_string(),
            start_line,
            line_count,
            start_char,
            max_chars: Some(effective_max_chars),
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

    let content = format!(
        "{} chars / {} words from {}, sha256 {}{}{}\n{}",
        read.chars,
        read.words,
        read.resource_ref.as_str(),
        read.sha256.as_str(),
        if read.truncated { " (truncated)" } else { "" },
        if read.start_line > 0 {
            format!(
                ", lines {}-{} of {}",
                read.start_line, read.end_line, read.total_lines
            )
        } else {
            format!(
                ", chars {}-{} of {}",
                read.start_char, read.end_char, read.total_chars
            )
        },
        read.content
    );
    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: structured_value(SkillReadStructured {
                name: read.name.as_str(),
                path: read.path.as_str(),
                sha256: read.sha256.as_str(),
                range: TextRangeMetricsPayload::new(
                    TextMetrics {
                        chars: read.chars,
                        words: read.words,
                    },
                    TextMetrics {
                        chars: read.total_chars,
                        words: read.total_words,
                    },
                    read.start_char,
                    read.end_char,
                ),
                total_lines: read.total_lines,
                start_line: read.start_line,
                end_line: read.end_line,
                resource_ref: read.resource_ref.as_str(),
            }),
            is_error: false,
            error_code: None,
            resource_refs: vec![read.resource_ref],
        },
        AgentToolEffect::None,
    ))
}
