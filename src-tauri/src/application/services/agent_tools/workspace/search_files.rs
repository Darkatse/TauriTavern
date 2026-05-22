use serde::Serialize;

use super::args::{
    ensure_visible_workspace_path, object_args, optional_list_path_arg, optional_usize_arg,
    required_trimmed_string_arg, tool_error,
};
use super::policy::{WorkspaceAccessPolicy, workspace_access_policy};
use super::render::filter_visible_entries;
use super::{MAX_SEARCH_CONTEXT_LINES, MAX_SEARCH_DEPTH, MAX_SEARCH_FILES, MAX_SEARCH_LIMIT};
use crate::application::errors::ApplicationError;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};
use crate::domain::repositories::workspace_repository::{
    WorkspaceEntryKind, WorkspaceFile, WorkspaceRepository,
};
use crate::domain::text_metrics::TextMetrics;
use crate::domain::text_search::PreparedTextSearch;

use super::super::dispatcher::AgentToolEffect;
use super::super::structured::{TextMetricsPayload, structured_value};

const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_SEARCH_CONTEXT_LINES: usize = 2;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSearchFilesStructured<'a> {
    query: &'a str,
    hits: Vec<WorkspaceSearchHitStructured<'a>>,
    searched_files: usize,
    skipped_files: usize,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSearchHitStructured<'a> {
    path: &'a str,
    score: f32,
    start_line: usize,
    end_line: usize,
    snippet: &'a str,
    #[serde(flatten)]
    metrics: TextMetricsPayload,
    sha256: &'a str,
    #[serde(rename = "ref")]
    ref_id: &'a str,
}

struct WorkspaceSearchHit {
    path: String,
    score: f32,
    start_line: usize,
    end_line: usize,
    snippet: String,
    chars: usize,
    words: usize,
    sha256: String,
    ref_id: String,
}

pub(in crate::application::services::agent_tools) async fn search_files(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
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
    let Some(query) = required_trimmed_string_arg(args, "query") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "query is required"),
            AgentToolEffect::None,
        ));
    };
    let path = match optional_list_path_arg(args, "path") {
        Ok(path) => path,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if let Some(path) = &path {
        if let Err(result) = ensure_visible_workspace_path(call, &policy, path) {
            return Ok((result, AgentToolEffect::None));
        }
    }

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
            tool_error(call, "workspace.search_limit_invalid", "limit must be >= 1"),
            AgentToolEffect::None,
        ));
    }
    if limit > MAX_SEARCH_LIMIT {
        return Ok((
            tool_error(
                call,
                "workspace.search_limit_too_large",
                &format!("limit must be <= {MAX_SEARCH_LIMIT}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let context_lines = match optional_usize_arg(args, "context_lines") {
        Ok(context_lines) => context_lines.unwrap_or(DEFAULT_SEARCH_CONTEXT_LINES),
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if context_lines > MAX_SEARCH_CONTEXT_LINES {
        return Ok((
            tool_error(
                call,
                "workspace.search_context_too_large",
                &format!("context_lines must be <= {MAX_SEARCH_CONTEXT_LINES}"),
            ),
            AgentToolEffect::None,
        ));
    }

    let (files, traversal_truncated) =
        match collect_search_files(workspace_repository, run_id, &policy, path.as_ref()).await {
            Ok(result) => result,
            Err(ApplicationError::ValidationError(message)) => {
                return Ok((
                    tool_error(call, "workspace.path_not_found", &message),
                    AgentToolEffect::None,
                ));
            }
            Err(error) => return Err(error),
        };
    let searched_files = files.len();
    let search = PreparedTextSearch::new(query, limit, context_lines);
    let mut hits = Vec::new();
    for file in files {
        hits.extend(search.search(&file.text).into_iter().map(|hit| {
            let path = file.path.as_str().to_string();
            let metrics = TextMetrics::from_text(&hit.snippet);
            WorkspaceSearchHit {
                ref_id: format!("workspace:{path}#L{}-L{}", hit.start_line, hit.end_line),
                path,
                score: hit.score,
                start_line: hit.start_line,
                end_line: hit.end_line,
                snippet: hit.snippet,
                chars: metrics.chars,
                words: metrics.words,
                sha256: file.sha256.clone(),
            }
        }));
    }

    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start_line.cmp(&right.start_line))
    });
    let hit_truncated = hits.len() > limit;
    hits.truncate(limit);

    let content = render_content(query, &hits, traversal_truncated || hit_truncated);
    let resource_refs = hits
        .iter()
        .map(|hit| hit.ref_id.clone())
        .collect::<Vec<_>>();
    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: structured_value(WorkspaceSearchFilesStructured {
                query,
                hits: hits.iter().map(structured_hit).collect(),
                searched_files,
                skipped_files: 0,
                truncated: traversal_truncated || hit_truncated,
            }),
            is_error: false,
            error_code: None,
            resource_refs,
        },
        AgentToolEffect::None,
    ))
}

async fn collect_search_files(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    policy: &WorkspaceAccessPolicy,
    path: Option<&WorkspacePath>,
) -> Result<(Vec<WorkspaceFile>, bool), ApplicationError> {
    let roots = match path {
        Some(path) => vec![path.clone()],
        None => policy
            .visible_roots
            .iter()
            .map(WorkspacePath::parse)
            .collect::<Result<Vec<_>, _>>()?,
    };

    let mut files = Vec::new();
    let mut truncated = false;
    for root in roots {
        let remaining = MAX_SEARCH_FILES.saturating_sub(files.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        let list = match workspace_repository
            .list_files(run_id, Some(&root), MAX_SEARCH_DEPTH, remaining + 1)
            .await
        {
            Ok(list) => filter_visible_entries(list, policy),
            Err(DomainError::NotFound(message)) => {
                return Err(ApplicationError::ValidationError(message));
            }
            Err(error) => return Err(error.into()),
        };
        truncated |= list.truncated;
        for entry in list.entries {
            if entry.kind != WorkspaceEntryKind::File {
                continue;
            }
            if files.len() >= MAX_SEARCH_FILES {
                truncated = true;
                break;
            }
            let file = match workspace_repository.read_text(run_id, &entry.path).await {
                Ok(file) => file,
                Err(DomainError::NotFound(message)) => {
                    return Err(ApplicationError::ValidationError(message));
                }
                Err(error) => return Err(error.into()),
            };
            files.push(file);
        }
    }
    Ok((files, truncated))
}

fn render_content(query: &str, hits: &[WorkspaceSearchHit], truncated: bool) -> String {
    if hits.is_empty() {
        return format!("No visible workspace files matched `{query}`.");
    }

    let mut content = format!(
        "Search `{query}` matched {} workspace location{}. Use workspace_read_file with path and start_line/line_count to read exact text.",
        hits.len(),
        if hits.len() == 1 { "" } else { "s" }
    );
    for hit in hits {
        content.push_str(&format!(
            "\n\n{} score {:.3} ref {}\n{}",
            hit.path, hit.score, hit.ref_id, hit.snippet
        ));
    }
    if truncated {
        content.push_str("\n\nResults were truncated.");
    }
    content
}

fn structured_hit(hit: &WorkspaceSearchHit) -> WorkspaceSearchHitStructured<'_> {
    WorkspaceSearchHitStructured {
        path: hit.path.as_str(),
        score: hit.score,
        start_line: hit.start_line,
        end_line: hit.end_line,
        snippet: hit.snippet.as_str(),
        metrics: TextMetricsPayload {
            chars: hit.chars,
            words: hit.words,
        },
        sha256: hit.sha256.as_str(),
        ref_id: hit.ref_id.as_str(),
    }
}
