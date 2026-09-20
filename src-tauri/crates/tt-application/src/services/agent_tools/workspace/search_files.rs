use serde::Serialize;
use serde_json::{Map, Value};

use super::args::{
    ensure_visible_workspace_path, optional_list_path_arg, optional_usize_arg,
    required_trimmed_string_arg, tool_error,
};
use super::{MAX_SEARCH_CONTEXT_LINES, MAX_SEARCH_DEPTH, MAX_SEARCH_FILES, MAX_SEARCH_LIMIT};
use crate::errors::ApplicationError;
use crate::services::agent_workspace_scope::ScopedWorkspaceFs;
use crate::services::agent_workspace_scope::WorkspaceAccessPolicy;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentToolResult, WorkspacePath};
use tt_domain::models::tool::ToolInvocation;
use tt_domain::text_metrics::TextMetrics;
use tt_domain::text_search::PreparedTextSearch;
use tt_ports::workspace_fs::{WorkspaceEntryKind, WorkspaceFs};

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

pub(in crate::services::agent_tools) async fn search_files(
    workspace: &ScopedWorkspaceFs,
    call: &ToolInvocation,
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let policy = &workspace.policy;
    let workspace_files: &dyn WorkspaceFs = workspace;
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
    if let Some(path) = &path
        && let Err(error) = ensure_visible_workspace_path(policy, path)
    {
        return Ok((error.into_tool_result(call), AgentToolEffect::None));
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
        match collect_search_paths(workspace_files, policy, path.as_ref()).await {
            Ok(result) => result,
            Err(DomainError::NotFound(message)) => {
                return Ok((
                    tool_error(call, "workspace.path_not_found", &message),
                    AgentToolEffect::None,
                ));
            }
            Err(error) => match super::args::classify_workspace_io_error(call, error) {
                Ok(result) => return Ok((result, AgentToolEffect::None)),
                Err(error) => return Err(error.into()),
            },
        };
    let mut searched_files = 0;
    let mut skipped_files = 0;
    let search = PreparedTextSearch::new(query, limit, context_lines);
    let mut hits = Vec::new();
    for path in files {
        let file = match workspace_files.read_text(&path).await {
            Ok(file) => file,
            Err(DomainError::WorkspaceFileNotText { .. }) => {
                skipped_files += 1;
                continue;
            }
            Err(DomainError::NotFound(message)) => {
                return Ok((
                    tool_error(call, "workspace.path_not_found", &message),
                    AgentToolEffect::None,
                ));
            }
            Err(error) => match super::args::classify_workspace_io_error(call, error) {
                Ok(result) => return Ok((result, AgentToolEffect::None)),
                Err(error) => return Err(error.into()),
            },
        };
        searched_files += 1;
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

    let mut content = render_content(query, &hits, traversal_truncated || hit_truncated);
    if skipped_files > 0 {
        content.push_str(&format!(
            "\n\nSkipped {skipped_files} file{} that could not be read as text.",
            if skipped_files == 1 { "" } else { "s" }
        ));
    }
    let resource_refs = hits
        .iter()
        .map(|hit| hit.ref_id.clone())
        .collect::<Vec<_>>();
    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: structured_value(WorkspaceSearchFilesStructured {
                query,
                hits: hits.iter().map(structured_hit).collect(),
                searched_files,
                skipped_files,
                truncated: traversal_truncated || hit_truncated,
            }),
            is_error: false,
            error_code: None,
            resource_refs,
        },
        AgentToolEffect::None,
    ))
}

async fn collect_search_paths(
    workspace_files: &dyn WorkspaceFs,
    policy: &WorkspaceAccessPolicy,
    path: Option<&WorkspacePath>,
) -> Result<(Vec<WorkspacePath>, bool), DomainError> {
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
        let list = workspace_files
            .list_files(Some(&root), MAX_SEARCH_DEPTH, remaining + 1)
            .await?;
        truncated |= list.truncated;
        for entry in list.entries {
            if entry.kind != WorkspaceEntryKind::File {
                continue;
            }
            if files.len() >= MAX_SEARCH_FILES {
                truncated = true;
                break;
            }
            files.push(entry.path);
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
