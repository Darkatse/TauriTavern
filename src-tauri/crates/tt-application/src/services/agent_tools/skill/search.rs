use serde::Serialize;

use super::super::common::{
    object_args, optional_usize_arg, required_trimmed_string_arg, tool_error,
};
use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::list::skill_is_visible;
use crate::errors::ApplicationError;
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentToolCall, AgentToolResult};
use tt_domain::models::skill::{SkillSearchHit, SkillSearchRequest};
use tt_domain::text_metrics::TextMetrics;

use super::super::structured::{TextMetricsPayload, structured_value};

const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 50;
const DEFAULT_CONTEXT_LINES: usize = 2;
const MAX_CONTEXT_LINES: usize = 5;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillSearchStructured<'a> {
    name: &'a str,
    query: &'a str,
    hits: Vec<SkillSearchHitStructured<'a>>,
    searched_files: usize,
    skipped_files: usize,
    truncated: bool,
    returned_chars: usize,
    returned_words: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillSearchHitStructured<'a> {
    path: &'a str,
    score: f32,
    start_line: usize,
    end_line: usize,
    snippet: &'a str,
    #[serde(flatten)]
    metrics: TextMetricsPayload,
    sha256: &'a str,
    resource_ref: &'a str,
}

pub(in crate::services::agent_tools) async fn search(
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
    let Some(query) = required_trimmed_string_arg(args, "query") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "query is required"),
            AgentToolEffect::None,
        ));
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

    let limit = match optional_usize_arg(args, "limit") {
        Ok(limit) => limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if limit == 0 {
        return Ok((
            tool_error(call, "skill.search_limit_invalid", "limit must be >= 1"),
            AgentToolEffect::None,
        ));
    }
    if limit > MAX_SEARCH_LIMIT {
        return Ok((
            tool_error(
                call,
                "skill.search_limit_too_large",
                &format!("limit must be <= {MAX_SEARCH_LIMIT}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let context_lines = match optional_usize_arg(args, "context_lines") {
        Ok(context_lines) => context_lines.unwrap_or(DEFAULT_CONTEXT_LINES),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if context_lines > MAX_CONTEXT_LINES {
        return Ok((
            tool_error(
                call,
                "skill.search_context_too_large",
                &format!("context_lines must be <= {MAX_CONTEXT_LINES}"),
            ),
            AgentToolEffect::None,
        ));
    }

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

    let path = args
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let search = match skill_service
        .search_skill_files(SkillSearchRequest {
            scope,
            name: name.to_string(),
            query: query.to_string(),
            path,
            limit,
            context_lines,
        })
        .await
    {
        Ok(search) => search,
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
    if search.returned_chars > remaining {
        return Ok((
            tool_error(
                call,
                "skill.read_budget_exhausted",
                &format!(
                    "search snippets exceed remaining run skill read budget of {remaining}; lower limit or context_lines"
                ),
            ),
            AgentToolEffect::None,
        ));
    }
    session.remember_skill_read_chars(search.returned_chars);

    let content = render_content(&search);
    let resource_refs = search
        .hits
        .iter()
        .map(|hit| hit.resource_ref.clone())
        .collect::<Vec<_>>();
    let returned_words = search
        .hits
        .iter()
        .map(|hit| TextMetrics::from_text(&hit.snippet).words)
        .sum::<usize>();
    let structured = structured_value(SkillSearchStructured {
        name: search.name.as_str(),
        query: search.query.as_str(),
        hits: search.hits.iter().map(structured_hit).collect(),
        searched_files: search.searched_files,
        skipped_files: search.skipped_files,
        truncated: search.truncated,
        returned_chars: search.returned_chars,
        returned_words,
    });
    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured,
            is_error: false,
            error_code: None,
            resource_refs,
        },
        AgentToolEffect::None,
    ))
}

fn structured_hit(hit: &SkillSearchHit) -> SkillSearchHitStructured<'_> {
    let metrics = TextMetrics::from_text(&hit.snippet);
    SkillSearchHitStructured {
        path: hit.path.as_str(),
        score: hit.score,
        start_line: hit.start_line,
        end_line: hit.end_line,
        snippet: hit.snippet.as_str(),
        metrics: metrics.into(),
        sha256: hit.sha256.as_str(),
        resource_ref: hit.resource_ref.as_str(),
    }
}

fn render_content(search: &tt_domain::models::skill::SkillSearchResult) -> String {
    if search.hits.is_empty() {
        return format!(
            "No files in Skill `{}` matched `{}`.",
            search.name, search.query
        );
    }

    let mut content = format!(
        "Search `{}` matched {} location{} in Skill `{}`. Use skill_read with path and start_line/line_count to read exact text.",
        search.query,
        search.hits.len(),
        if search.hits.len() == 1 { "" } else { "s" },
        search.name
    );
    for hit in &search.hits {
        content.push_str(&format!(
            "\n\n{} score {:.3} ref {}\n{}",
            hit.path, hit.score, hit.resource_ref, hit.snippet
        ));
    }
    if search.truncated {
        content.push_str("\n\nResults were truncated.");
    }
    content
}
