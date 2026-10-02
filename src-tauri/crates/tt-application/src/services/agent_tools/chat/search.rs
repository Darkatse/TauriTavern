use serde::Serialize;
use serde_json::{Map, Value};

use super::{DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT, MAX_SEARCH_SCAN_LIMIT, chat_read_error};
use crate::errors::ApplicationError;
use crate::services::agent_tools::common::{
    optional_bool_arg, optional_usize_arg, required_trimmed_string_arg, tool_error,
};
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use crate::services::agent_workspace_scope::{
    ChatFloor, ChatSnapshot, FloorRole, floor_message_path,
};
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;
use tt_domain::text_metrics::TextMetrics;
use tt_domain::text_search::{RankedHit, RankedTextSearch};

use super::super::structured::{TextMetricsPayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatSearchStructured<'a> {
    query: &'a str,
    hits: Vec<ChatSearchHitStructured<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatSearchHitStructured<'a> {
    index: usize,
    #[serde(flatten)]
    role: FloorRole,
    score: f32,
    snippet: &'a str,
    #[serde(flatten)]
    metrics: TextMetricsPayload,
    #[serde(rename = "ref")]
    ref_id: String,
    /// The floor file in the chat mount; a group chat is not mounted.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

pub(in crate::services::agent_tools) async fn search(
    chat: &ChatSnapshot,
    call: &ToolInvocation,
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let query = match required_trimmed_string_arg(args, "query") {
        Some(query) => query.to_string(),
        None => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", "query is required"),
                AgentToolEffect::None,
            ));
        }
    };
    let search_query = match parse_search(args, query) {
        Ok(query) => query,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    // A mounted chat has floor files, so hits point at the file to read.
    let floor_files = chat.is_mounted();
    let hits = match chat.floors().await {
        Ok(floors) => search_floors(floors, &search_query),
        Err(error) => return chat_read_error(call, error),
    };

    let content = render_content(&search_query.query, &hits, floor_files);
    let resource_refs = hits
        .iter()
        .map(|hit| {
            if floor_files {
                format!("workspace:{}", floor_message_path(hit.index))
            } else {
                format!("chat:current#{}", hit.index)
            }
        })
        .collect::<Vec<_>>();

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: structured_value(ChatSearchStructured {
                query: search_query.query.as_str(),
                hits: hits
                    .iter()
                    .map(|hit| structured_hit(hit, floor_files))
                    .collect(),
            }),
            is_error: false,
            error_code: None,
            resource_refs,
        },
        AgentToolEffect::None,
    ))
}

/// A search over the run's floors, as the model asked for it.
struct FloorSearch {
    query: String,
    limit: usize,
    /// A [`FloorRole::role`], filtered as the floors show it.
    role: Option<&'static str>,
    /// Keeps only hidden floors (`true`) or only floors in the prompt (`false`).
    hidden: Option<bool>,
    start_floor: Option<usize>,
    end_floor: Option<usize>,
    scan_limit: Option<usize>,
}

/// Ranks the run's floors as the chat search API ranks a chat file: within the floor
/// range and the `scan_limit` most recent floors, of the chosen role and visibility as
/// the floors show them. A floor without text has no words to match.
fn search_floors<'a>(
    floors: &'a [ChatFloor],
    search: &FloorSearch,
) -> Vec<RankedHit<'a, FloorRole>> {
    let total = floors.len();
    let scanned = search.scan_limit.unwrap_or(total).min(total);
    let start = search.start_floor.unwrap_or(0).max(total - scanned);
    let end = search
        .end_floor
        .map_or(total, |end| end.saturating_add(1))
        .min(total);

    let mut ranked = RankedTextSearch::new(&search.query, search.limit);
    for (index, floor) in floors.iter().enumerate().take(end).skip(start) {
        if search.role.is_some_and(|role| role != floor.role.role)
            || search
                .hidden
                .is_some_and(|hidden| hidden != floor.role.hidden)
        {
            continue;
        }
        if let Some(text) = floor.message.as_deref() {
            ranked.offer(index, text, floor.role);
        }
    }
    ranked.finish()
}

fn parse_search(args: &Map<String, Value>, query: String) -> Result<FloorSearch, String> {
    let limit = optional_usize_arg(args, "limit")?.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if limit == 0 {
        return Err("limit must be >= 1".to_string());
    }
    if limit > MAX_SEARCH_LIMIT {
        return Err(format!("limit must be <= {MAX_SEARCH_LIMIT}"));
    }

    let role = match args.get("role") {
        Some(Value::String(value)) => Some(parse_floor_role(value)?),
        Some(_) => return Err("role must be a string".to_string()),
        None => None,
    };
    let hidden = optional_bool_arg(args, "hidden")?;
    let start_floor = optional_usize_arg(args, "start_floor")?;
    let end_floor = optional_usize_arg(args, "end_floor")?;
    if matches!((start_floor, end_floor), (Some(start), Some(end)) if start > end) {
        return Err("start_floor must be <= end_floor".to_string());
    }
    let scan_limit = optional_usize_arg(args, "scan_limit")?;
    if scan_limit == Some(0) {
        return Err("scan_limit must be >= 1".to_string());
    }
    if scan_limit.is_some_and(|value| value > MAX_SEARCH_SCAN_LIMIT) {
        return Err(format!("scan_limit must be <= {MAX_SEARCH_SCAN_LIMIT}"));
    }

    Ok(FloorSearch {
        query,
        limit,
        role,
        hidden,
        start_floor,
        end_floor,
        scan_limit,
    })
}

/// A role as the floors show it. `system` is how the chat search API lists hidden floors;
/// a run frozen with that schema is told how to select them now.
fn parse_floor_role(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Ok("user"),
        "assistant" => Ok("assistant"),
        "tool" => Ok("tool"),
        "system" => Err(
            "role system is not a floor role; hidden floors keep their role, so select them with hidden: true."
                .to_string(),
        ),
        _ => Err("role must be user, assistant, or tool".to_string()),
    }
}

fn render_content(query: &str, hits: &[RankedHit<'_, FloorRole>], floor_files: bool) -> String {
    if hits.is_empty() {
        return format!("No floors matched `{query}` in the current chat.");
    }

    let mut content = format!(
        "Search `{query}` matched {} floor{} in the current chat. {}",
        hits.len(),
        if hits.len() == 1 { "" } else { "s" },
        if floor_files {
            "Read the floor file for the exact text."
        } else {
            "Read these floors by index for the exact text."
        }
    );
    for hit in hits {
        let location = if floor_files {
            floor_message_path(hit.index)
        } else {
            format!("ref chat:current#{}", hit.index)
        };
        content.push_str(&format!(
            "\n\nfloor {} {} score {:.3} {location}\n{}",
            hit.index, hit.item, hit.score, hit.snippet
        ));
    }
    content
}

fn structured_hit<'a>(
    hit: &'a RankedHit<'_, FloorRole>,
    floor_files: bool,
) -> ChatSearchHitStructured<'a> {
    let metrics = TextMetrics::from_text(&hit.snippet);
    ChatSearchHitStructured {
        index: hit.index,
        role: hit.item,
        score: hit.score,
        snippet: hit.snippet.as_str(),
        metrics: metrics.into(),
        ref_id: format!("chat:current#{}", hit.index),
        path: floor_files.then(|| floor_message_path(hit.index)),
    }
}
