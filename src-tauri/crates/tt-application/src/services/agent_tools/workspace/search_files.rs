use std::borrow::Cow;

use regex::RegexBuilder;
use serde::Serialize;
use serde_json::{Map, Value};

use super::args::{
    ensure_visible_workspace_path, optional_list_path_arg, required_raw_string_arg, tool_error,
};
use super::{MAX_SEARCH_DEPTH, MAX_SEARCH_FILES};
use crate::errors::ApplicationError;
use crate::services::agent_workspace_scope::{
    AGENT_TOOL_RESULTS_ROOT, ChatText, HIDDEN_MARK, SKILLS_ROOT, ScopedWorkspaceFs,
    WorkspaceAccessPolicy, is_chat_mount_path,
};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentToolResult, WorkspacePath};
use tt_domain::models::tool::ToolInvocation;
use tt_ports::workspace_fs::{WorkspaceEntryKind, WorkspaceFs};

use super::super::dispatcher::AgentToolEffect;
use super::super::structured::structured_value;

/// Matching lines shown; the count in the result covers all of them.
const MAX_MATCHES: usize = 100;
/// Characters kept from a long line, around its first match.
const MAX_LINE_CHARS: usize = 300;
/// Roots with their own entry points, searched only when `path` names them.
const EXPLICIT_ONLY_ROOTS: [&str; 2] = [AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT];

#[derive(Serialize)]
struct WorkspaceGrepStructured<'a> {
    matches: &'a [GrepMatch],
    truncated: bool,
}

#[derive(Serialize)]
struct GrepMatch {
    path: String,
    line: usize,
    text: String,
    /// `is_system` floor: hidden from the prompt by the user, still part of the chat.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
}

enum Source<'a> {
    File(WorkspacePath),
    Chat(ChatText<'a>),
}

impl Source<'_> {
    fn path(&self) -> &str {
        match self {
            Source::File(path) => path.as_str(),
            Source::Chat(text) => &text.path,
        }
    }
}

/// Line-based regex search. Without `path` it covers the visible roots except
/// `tool-results/` and `skills/`, plus every floor's `message.md`; with `path` it covers
/// that file or subtree only. Chat files are matched in the mount's snapshot, not
/// traversed as files, so a long chat spends no file budget.
pub(in crate::services::agent_tools) async fn search_files(
    workspace: &ScopedWorkspaceFs,
    call: &ToolInvocation,
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let policy = &workspace.policy;
    let workspace_files: &dyn WorkspaceFs = workspace;
    let Some(pattern) = required_raw_string_arg(args, "pattern").filter(|value| !value.is_empty())
    else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "pattern is required"),
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
    // The regex crate matches in linear time; its default compiled-size limit applies.
    let regex = match RegexBuilder::new(pattern).build() {
        Ok(regex) => regex,
        Err(error) => {
            return Ok((
                tool_error(
                    call,
                    "workspace.grep_pattern_invalid",
                    &format!("Invalid regular expression: {error}"),
                ),
                AgentToolEffect::None,
            ));
        }
    };

    let in_chat_mount = path.as_ref().is_some_and(is_chat_mount_path);
    let (files, traversal_truncated) = if in_chat_mount {
        (Vec::new(), false)
    } else {
        match collect_search_paths(workspace_files, policy, path.as_ref()).await {
            Ok(result) => result,
            Err(error) => return error_result(call, error),
        }
    };
    let chat = if path.is_none() || in_chat_mount {
        match workspace.chat_texts(path.as_ref()).await {
            Ok(texts) => texts,
            Err(error) => return error_result(call, error),
        }
    } else {
        Vec::new()
    };
    let mut sources = files
        .into_iter()
        .map(Source::File)
        .chain(chat.into_iter().map(Source::Chat))
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| left.path().cmp(right.path()));

    let mut matches = Vec::new();
    let mut total = 0;
    let mut skipped_files = 0;
    let mut skipped_floors = 0;
    for source in &sources {
        let (text, hidden) = match source {
            Source::Chat(chat) => match chat.text {
                Some(text) => (Cow::Borrowed(text), chat.hidden),
                // Reading the floor's message.md says why it has no text.
                None => {
                    skipped_floors += 1;
                    continue;
                }
            },
            Source::File(path) => match workspace_files.read_file(path, usize::MAX).await {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => (Cow::Owned(text), false),
                    // Binary files hold no lines to match; the result says how many.
                    Err(_) => {
                        skipped_files += 1;
                        continue;
                    }
                },
                Err(error) => return error_result(call, error),
            },
        };
        for (index, line) in text.lines().enumerate() {
            let Some(found) = regex.find(line) else {
                continue;
            };
            total += 1;
            if matches.len() < MAX_MATCHES {
                matches.push(GrepMatch {
                    path: source.path().to_owned(),
                    line: index + 1,
                    text: excerpt(line, found.start(), found.end()),
                    hidden,
                });
            }
        }
    }

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content: {
                let mut content = render_content(pattern, &matches, total, traversal_truncated);
                if skipped_files > 0 {
                    content.push_str(&format!(
                        "\n\nSkipped {skipped_files} file{} that could not be read as text.",
                        if skipped_files == 1 { "" } else { "s" }
                    ));
                }
                if skipped_floors > 0 {
                    content.push_str(&format!(
                        "\n\nSkipped {skipped_floors} floor{} without message text; reading its message.md gives the cause.",
                        if skipped_floors == 1 { "" } else { "s" }
                    ));
                }
                content
            },
            structured: structured_value(WorkspaceGrepStructured {
                matches: &matches,
                truncated: total > matches.len() || traversal_truncated,
            }),
            is_error: false,
            error_code: None,
            resource_refs: matches
                .iter()
                .map(|found| format!("workspace:{}#L{}-L{}", found.path, found.line, found.line))
                .collect(),
        },
        AgentToolEffect::None,
    ))
}

fn error_result(
    call: &ToolInvocation,
    error: DomainError,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let result = match error {
        DomainError::NotFound(message) => tool_error(call, "workspace.path_not_found", &message),
        error => super::args::classify_workspace_io_error(call, error)?,
    };
    Ok((result, AgentToolEffect::None))
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
            .filter(|root| !EXPLICIT_ONLY_ROOTS.contains(&root.as_str()))
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

/// A long line cut on character boundaries to about `MAX_LINE_CHARS` around its first
/// match, with `…` where text was dropped.
fn excerpt(line: &str, start: usize, end: usize) -> String {
    let total = line.chars().count();
    if total <= MAX_LINE_CHARS {
        return line.to_owned();
    }
    let match_start = line[..start].chars().count();
    let match_chars = line[start..end].chars().count().min(MAX_LINE_CHARS);
    let from = match_start
        .saturating_sub((MAX_LINE_CHARS - match_chars) / 2)
        .min(total - MAX_LINE_CHARS);
    let kept = line
        .chars()
        .skip(from)
        .take(MAX_LINE_CHARS)
        .collect::<String>();
    format!(
        "{}{kept}{}",
        if from > 0 { "\u{2026}" } else { "" },
        if from + MAX_LINE_CHARS < total {
            "\u{2026}"
        } else {
            ""
        }
    )
}

fn render_content(
    pattern: &str,
    matches: &[GrepMatch],
    total: usize,
    traversal_truncated: bool,
) -> String {
    let mut content = if total == 0 {
        format!("No matches for `{pattern}`.")
    } else {
        format!("Found {total} match{}", if total == 1 { "" } else { "es" })
    };
    let mut current = None;
    for found in matches {
        if current != Some(found.path.as_str()) {
            current = Some(found.path.as_str());
            content.push_str(&format!(
                "\n\n{}{}",
                found.path,
                if found.hidden { HIDDEN_MARK } else { "" }
            ));
        }
        content.push_str(&format!("\nLine {}: {}", found.line, found.text));
    }
    if total > matches.len() {
        content.push_str(&format!(
            "\n\nShowing the first {} of {total} matches; narrow the pattern or path.",
            matches.len()
        ));
    }
    if traversal_truncated {
        content.push_str(&format!(
            "\n\nOnly the first {MAX_SEARCH_FILES} files were searched; narrow the path."
        ));
    }
    content
}
