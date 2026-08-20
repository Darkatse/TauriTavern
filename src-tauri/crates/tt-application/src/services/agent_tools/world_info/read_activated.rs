use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    MAX_WORLDINFO_ENTRIES_PER_READ, MAX_WORLDINFO_ENTRY_READ_CHARS, MAX_WORLDINFO_ENTRY_READ_LINES,
    MAX_WORLDINFO_TOTAL_READ_CHARS,
};
use crate::errors::ApplicationError;
use crate::services::agent_tools::common::{object_args, tool_error};
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;
use tt_domain::text_lines::TextLineSelection;
use tt_domain::text_metrics::TextMetrics;

use super::super::structured::{TextLineRangePayload, TextTotalMetricsPayload, structured_value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldInfoIndexStructured<'a> {
    mode: &'static str,
    timestamp_ms: Option<i64>,
    trigger: Option<&'a str>,
    total_entries: usize,
    entries: Vec<WorldInfoIndexEntryStructured<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldInfoIndexEntryStructured<'a> {
    world: &'a str,
    uid: &'a str,
    display_name: Option<&'a str>,
    constant: bool,
    position: Option<&'a str>,
    #[serde(flatten)]
    metrics: TextTotalMetricsPayload,
    total_lines: usize,
    #[serde(rename = "ref")]
    ref_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldInfoContentStructured<'a> {
    mode: &'static str,
    timestamp_ms: Option<i64>,
    trigger: Option<&'a str>,
    total_entries: usize,
    entries: Vec<WorldInfoContentEntryStructured<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldInfoContentEntryStructured<'a> {
    world: &'a str,
    uid: &'a str,
    display_name: Option<&'a str>,
    constant: bool,
    position: Option<&'a str>,
    #[serde(flatten)]
    range: TextLineRangePayload,
    content: &'a str,
    #[serde(rename = "ref")]
    ref_id: &'a str,
}

enum ReadActivatedRequest {
    Index,
    Content(Vec<EntryContentRequest>),
}

#[derive(Debug)]
struct EntryContentRequest {
    ref_id: String,
    start_line: Option<usize>,
    line_count: Option<usize>,
}

pub(in crate::services::agent_tools) struct ActivatedEntry {
    pub(in crate::services::agent_tools) world: String,
    pub(in crate::services::agent_tools) uid: String,
    pub(in crate::services::agent_tools) display_name: Option<String>,
    pub(in crate::services::agent_tools) constant: bool,
    pub(in crate::services::agent_tools) position: Option<String>,
    pub(in crate::services::agent_tools) content: String,
    pub(in crate::services::agent_tools) metrics: TextMetrics,
    pub(in crate::services::agent_tools) total_lines: usize,
    pub(in crate::services::agent_tools) ref_id: String,
}

struct RenderedEntry {
    world: String,
    uid: String,
    display_name: Option<String>,
    constant: bool,
    position: Option<String>,
    selection: TextLineSelection,
    metrics: TextMetrics,
    total_metrics: TextMetrics,
    ref_id: String,
}

pub(in crate::services::agent_tools) fn read_activated(
    prompt_snapshot: &Value,
    call: &ToolInvocation,
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
    let request = match parse_request(args) {
        Ok(request) => request,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };

    let Some(batch) = prompt_snapshot.get("worldInfoActivation") else {
        return Ok((
            tool_error(
                call,
                "worldinfo.activation_unavailable",
                "this run has no worldInfoActivation snapshot",
            ),
            AgentToolEffect::None,
        ));
    };
    let entries = batch
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_activation_snapshot("entries must be an array"))?
        .iter()
        .enumerate()
        .map(|(index, entry)| normalize_entry(index, entry))
        .collect::<Result<Vec<_>, _>>()?;

    let result = match request {
        ReadActivatedRequest::Index => build_index_result(call, batch, &entries),
        ReadActivatedRequest::Content(requests) => {
            match build_content_result(call, batch, &entries, &requests) {
                Ok(result) => result,
                Err((code, message)) => tool_error(call, code, &message),
            }
        }
    };

    Ok((result, AgentToolEffect::None))
}

fn parse_request(args: &Map<String, Value>) -> Result<ReadActivatedRequest, String> {
    if args.is_empty() {
        return Ok(ReadActivatedRequest::Index);
    }

    for key in args.keys() {
        if key != "entries" {
            return Err(format!(
                "{key} is not supported; omit arguments to list active World Info entries, or pass entries to read selected content"
            ));
        }
    }

    let values = args
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "entries is required and must be an array".to_string())?;
    if values.is_empty() {
        return Err("entries must include at least one item".to_string());
    }
    if values.len() > MAX_WORLDINFO_ENTRIES_PER_READ {
        return Err(format!(
            "entries can include at most {MAX_WORLDINFO_ENTRIES_PER_READ} items"
        ));
    }

    values
        .iter()
        .enumerate()
        .map(|(position, value)| parse_entry_request(position, value))
        .collect::<Result<Vec<_>, _>>()
        .map(ReadActivatedRequest::Content)
}

fn parse_entry_request(position: usize, value: &Value) -> Result<EntryContentRequest, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("entries[{position}] must be an object"))?;
    for key in object.keys() {
        if key != "ref" && key != "start_line" && key != "line_count" {
            return Err(format!("entries[{position}].{key} is not supported"));
        }
    }

    let ref_id = object
        .get("ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("entries[{position}].ref is required"))?
        .to_string();

    Ok(EntryContentRequest {
        ref_id,
        start_line: optional_entry_usize(object, "start_line", position)?,
        line_count: optional_entry_usize(object, "line_count", position)?,
    })
}

fn optional_entry_usize(
    object: &Map<String, Value>,
    key: &str,
    position: usize,
) -> Result<Option<usize>, String> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let Some(value) = value.as_u64() else {
        return Err(format!(
            "entries[{position}].{key} must be a non-negative integer"
        ));
    };
    usize::try_from(value)
        .map(Some)
        .map_err(|_| format!("entries[{position}].{key} is too large"))
}

pub(in crate::services::agent_tools) fn normalize_entry(
    index: usize,
    entry: &Value,
) -> Result<ActivatedEntry, ApplicationError> {
    let entry = entry.as_object().ok_or_else(|| {
        invalid_activation_snapshot(format!("entries[{index}] must be an object"))
    })?;
    let world = entry
        .get("world")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let uid = match entry.get("uid") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    };
    let ref_id = if world.is_empty() || uid.is_empty() {
        format!("worldinfo:activated#{index}")
    } else {
        format!("worldinfo:{world}#{uid}")
    };
    let content = entry
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid_activation_snapshot(format!("entries[{index}].content must be a string"))
        })?
        .to_string();
    let metrics = TextMetrics::from_text(&content);
    let total_lines = if content.is_empty() {
        0
    } else {
        content.split('\n').count()
    };

    Ok(ActivatedEntry {
        world,
        uid,
        display_name: entry
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::to_string),
        constant: entry
            .get("constant")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        position: entry
            .get("position")
            .and_then(Value::as_str)
            .map(str::to_string),
        content,
        metrics,
        total_lines,
        ref_id,
    })
}

fn build_index_result(
    call: &ToolInvocation,
    batch: &Value,
    entries: &[ActivatedEntry],
) -> AgentToolResult {
    let resource_refs = entries
        .iter()
        .map(|entry| entry.ref_id.clone())
        .collect::<Vec<_>>();
    let content = render_index_content(entries);

    AgentToolResult {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        content,
        structured: structured_value(WorldInfoIndexStructured {
            mode: "index",
            timestamp_ms: batch.get("timestampMs").and_then(Value::as_i64),
            trigger: batch.get("trigger").and_then(Value::as_str),
            total_entries: entries.len(),
            entries: entries.iter().map(index_entry).collect(),
        }),
        is_error: false,
        error_code: None,
        resource_refs,
    }
}

fn build_content_result(
    call: &ToolInvocation,
    batch: &Value,
    entries: &[ActivatedEntry],
    requests: &[EntryContentRequest],
) -> Result<AgentToolResult, (&'static str, String)> {
    let per_entry_chars =
        MAX_WORLDINFO_ENTRY_READ_CHARS.min(MAX_WORLDINFO_TOTAL_READ_CHARS / requests.len());
    let mut rendered = Vec::with_capacity(requests.len());

    for request in requests {
        let Some(entry) = entries.iter().find(|entry| entry.ref_id == request.ref_id) else {
            return Err((
                "worldinfo.entry_not_found",
                format!(
                    "{} is not an active World Info ref in this run; call without arguments to list active refs",
                    request.ref_id
                ),
            ));
        };
        let item = render_entry(entry, request, per_entry_chars)
            .map_err(|message| ("worldinfo.invalid_entry_range", message))?;
        rendered.push(item);
    }

    let resource_refs = rendered
        .iter()
        .map(|entry| entry.ref_id.clone())
        .collect::<Vec<_>>();
    let content = render_content_entries(&rendered);

    Ok(AgentToolResult {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        content,
        structured: structured_value(WorldInfoContentStructured {
            mode: "content",
            timestamp_ms: batch.get("timestampMs").and_then(Value::as_i64),
            trigger: batch.get("trigger").and_then(Value::as_str),
            total_entries: entries.len(),
            entries: rendered.iter().map(content_entry).collect(),
        }),
        is_error: false,
        error_code: None,
        resource_refs,
    })
}

fn render_entry(
    entry: &ActivatedEntry,
    request: &EntryContentRequest,
    max_chars: usize,
) -> Result<RenderedEntry, String> {
    let selection = TextLineSelection::select(
        &entry.content,
        request.start_line.unwrap_or(1),
        request.line_count,
        MAX_WORLDINFO_ENTRY_READ_LINES,
        max_chars,
    )
    .map_err(|error| format!("{}: {error}", entry.ref_id))?;
    let metrics = TextMetrics::from_text(&selection.content);

    Ok(RenderedEntry {
        world: entry.world.clone(),
        uid: entry.uid.clone(),
        display_name: entry.display_name.clone(),
        constant: entry.constant,
        position: entry.position.clone(),
        selection,
        metrics,
        total_metrics: entry.metrics,
        ref_id: entry.ref_id.clone(),
    })
}

fn render_index_content(entries: &[ActivatedEntry]) -> String {
    if entries.is_empty() {
        return "No World Info entries were activated for this run.".to_string();
    }

    let mut content = format!(
        "Activated World Info for this run: {} entr{}. Content is omitted; call this tool with entries[].ref to read selected content.",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" }
    );
    for (index, entry) in entries.iter().enumerate() {
        content.push_str(&format!(
            "\n{}. {} | {} | world={} | lines={} | chars={} | words={}",
            index + 1,
            entry.ref_id,
            display_label(entry),
            entry.world,
            entry.total_lines,
            entry.metrics.chars,
            entry.metrics.words
        ));
        if let Some(position) = &entry.position {
            content.push_str(&format!(" | position={position}"));
        }
        if entry.constant {
            content.push_str(" | constant");
        }
    }
    content
}

fn render_content_entries(entries: &[RenderedEntry]) -> String {
    let mut content = format!(
        "Read {} activated World Info entr{}.",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" }
    );
    for entry in entries {
        content.push_str(&format!(
            "\n\n{} | {} | world={} | lines {}-{} of {} | chars {} of {} | words {} of {}{}",
            entry.ref_id,
            display_label_rendered(entry),
            entry.world,
            entry.selection.start_line,
            entry.selection.end_line,
            entry.selection.total_lines,
            entry.metrics.chars,
            entry.total_metrics.chars,
            entry.metrics.words,
            entry.total_metrics.words,
            if entry.selection.truncated() {
                " | preview"
            } else {
                ""
            }
        ));
        if let Some(position) = &entry.position {
            content.push_str(&format!(" | position={position}"));
        }
        let numbered = entry.selection.numbered_content();
        if !numbered.is_empty() {
            content.push('\n');
            content.push_str(&numbered);
        }
        if let Some(next_start_line) = entry.selection.next_start_line() {
            content.push_str(&format!(
                "\nContinue {} with start_line={next_start_line} and line_count={}.",
                entry.ref_id,
                entry.selection.returned_line_count()
            ));
        }
        if entry.selection.line_truncated {
            content.push_str(&format!(
                "\nLine {} exceeds the read preview budget and was truncated.",
                entry.selection.start_line
            ));
        }
    }
    content
}

fn index_entry(entry: &ActivatedEntry) -> WorldInfoIndexEntryStructured<'_> {
    WorldInfoIndexEntryStructured {
        world: entry.world.as_str(),
        uid: entry.uid.as_str(),
        display_name: entry.display_name.as_deref(),
        constant: entry.constant,
        position: entry.position.as_deref(),
        metrics: entry.metrics.into(),
        total_lines: entry.total_lines,
        ref_id: entry.ref_id.as_str(),
    }
}

fn content_entry(entry: &RenderedEntry) -> WorldInfoContentEntryStructured<'_> {
    WorldInfoContentEntryStructured {
        world: entry.world.as_str(),
        uid: entry.uid.as_str(),
        display_name: entry.display_name.as_deref(),
        constant: entry.constant,
        position: entry.position.as_deref(),
        range: TextLineRangePayload::new(
            entry.metrics,
            entry.total_metrics,
            entry.selection.total_lines,
            entry.selection.start_line,
            entry.selection.end_line,
            entry.selection.line_truncated,
        ),
        content: entry.selection.content.as_str(),
        ref_id: entry.ref_id.as_str(),
    }
}

fn display_label(entry: &ActivatedEntry) -> &str {
    entry
        .display_name
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(entry.uid.as_str())
}

fn display_label_rendered(entry: &RenderedEntry) -> &str {
    entry
        .display_name
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(entry.uid.as_str())
}

fn invalid_activation_snapshot(message: impl Into<String>) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.invalid_worldinfo_activation_snapshot: {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tt_domain::text_metrics::TextMetrics;

    use super::{
        ActivatedEntry, EntryContentRequest, MAX_WORLDINFO_ENTRY_READ_CHARS, parse_entry_request,
        render_entry,
    };

    #[test]
    fn long_entries_default_to_a_line_preview() {
        let content = format!("{}\n{}", "a".repeat(5_000), "b".repeat(5_000));
        let entry = ActivatedEntry {
            world: "world".to_string(),
            uid: "1".to_string(),
            display_name: None,
            constant: false,
            position: None,
            metrics: TextMetrics::from_text(&content),
            total_lines: 2,
            content,
            ref_id: "worldinfo:world#1".to_string(),
        };
        let rendered = render_entry(
            &entry,
            &EntryContentRequest {
                ref_id: entry.ref_id.clone(),
                start_line: None,
                line_count: None,
            },
            MAX_WORLDINFO_ENTRY_READ_CHARS,
        )
        .unwrap();

        assert_eq!(rendered.selection.end_line, 1);
        assert_eq!(rendered.selection.next_start_line(), Some(2));
        assert!(rendered.selection.truncated());
    }

    #[test]
    fn character_ranges_are_not_accepted() {
        let error = parse_entry_request(0, &json!({ "ref": "worldinfo:world#1", "max_chars": 10 }))
            .unwrap_err();
        assert!(error.contains("max_chars is not supported"));
    }
}
