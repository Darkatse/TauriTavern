use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    MAX_WORLDINFO_ENTRIES_PER_READ, MAX_WORLDINFO_ENTRY_RANGE_CHARS,
    MAX_WORLDINFO_FULL_ENTRY_CHARS, MAX_WORLDINFO_TOTAL_READ_CHARS,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::common::{object_args, tool_error};
use crate::application::services::agent_tools::dispatcher::AgentToolEffect;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};
use crate::domain::text_metrics::TextMetrics;

use super::super::structured::{
    TextRangeMetricsPayload, TextTotalMetricsPayload, structured_value,
};

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
    range: TextRangeMetricsPayload,
    content: &'a str,
    #[serde(rename = "ref")]
    ref_id: &'a str,
}

enum ReadActivatedRequest {
    Index,
    Content(Vec<EntryContentRequest>),
}

struct EntryContentRequest {
    ref_id: String,
    start_char: Option<usize>,
    max_chars: Option<usize>,
}

struct ActivatedEntry {
    world: String,
    uid: String,
    display_name: Option<String>,
    constant: bool,
    position: Option<String>,
    content: String,
    metrics: TextMetrics,
    ref_id: String,
}

struct RenderedEntry {
    world: String,
    uid: String,
    display_name: Option<String>,
    constant: bool,
    position: Option<String>,
    start_char: usize,
    end_char: usize,
    chars: usize,
    total_chars: usize,
    words: usize,
    total_words: usize,
    truncated: bool,
    content: String,
    ref_id: String,
}

pub(in crate::application::services::agent_tools) fn read_activated(
    prompt_snapshot: &Value,
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
        if key != "ref" && key != "start_char" && key != "max_chars" {
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
    let start_char = optional_entry_usize(object, "start_char", position)?;
    let max_chars = optional_entry_usize(object, "max_chars", position)?;
    if max_chars == Some(0) {
        return Err(format!("entries[{position}].max_chars must be >= 1"));
    }
    if max_chars.is_some_and(|value| value > MAX_WORLDINFO_ENTRY_RANGE_CHARS) {
        return Err(format!(
            "entries[{position}].max_chars must be <= {MAX_WORLDINFO_ENTRY_RANGE_CHARS}"
        ));
    }

    Ok(EntryContentRequest {
        ref_id,
        start_char,
        max_chars,
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

fn normalize_entry(index: usize, entry: &Value) -> Result<ActivatedEntry, ApplicationError> {
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
        ref_id,
    })
}

fn build_index_result(
    call: &AgentToolCall,
    batch: &Value,
    entries: &[ActivatedEntry],
) -> AgentToolResult {
    let resource_refs = entries
        .iter()
        .map(|entry| entry.ref_id.clone())
        .collect::<Vec<_>>();
    let content = render_index_content(entries);

    AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
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
    call: &AgentToolCall,
    batch: &Value,
    entries: &[ActivatedEntry],
    requests: &[EntryContentRequest],
) -> Result<AgentToolResult, (&'static str, String)> {
    let mut rendered = Vec::with_capacity(requests.len());
    let mut total_returned_chars = 0_usize;

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
        let item = render_entry(entry, request)
            .map_err(|message| ("worldinfo.invalid_entry_range", message))?;
        total_returned_chars += item.chars;
        if total_returned_chars > MAX_WORLDINFO_TOTAL_READ_CHARS {
            return Err((
                "worldinfo.read_too_large",
                format!(
                    "read result exceeds {MAX_WORLDINFO_TOTAL_READ_CHARS} characters; read fewer entries or smaller ranges"
                ),
            ));
        }
        rendered.push(item);
    }

    let resource_refs = rendered
        .iter()
        .map(|entry| entry.ref_id.clone())
        .collect::<Vec<_>>();
    let content = render_content_entries(&rendered);

    Ok(AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
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
) -> Result<RenderedEntry, String> {
    let total_chars = entry.metrics.chars;
    let start_char = request.start_char.unwrap_or(0);
    if total_chars > 0 && start_char >= total_chars {
        return Err(format!(
            "{} has {total_chars} characters; start_char {start_char} is outside the entry",
            entry.ref_id
        ));
    }
    if total_chars == 0 && start_char > 0 {
        return Err(format!("{} is empty; start_char must be 0", entry.ref_id));
    }
    if request.max_chars.is_none() && total_chars > MAX_WORLDINFO_FULL_ENTRY_CHARS {
        return Err(format!(
            "{} has {total_chars} characters; set start_char and max_chars to read it in ranges",
            entry.ref_id
        ));
    }

    let requested = request
        .max_chars
        .unwrap_or_else(|| total_chars.saturating_sub(start_char));
    let end_char = start_char.saturating_add(requested).min(total_chars);
    let content = slice_chars(&entry.content, start_char, end_char);
    let selected_metrics = TextMetrics::from_text(&content);

    Ok(RenderedEntry {
        world: entry.world.clone(),
        uid: entry.uid.clone(),
        display_name: entry.display_name.clone(),
        constant: entry.constant,
        position: entry.position.clone(),
        start_char,
        end_char,
        chars: selected_metrics.chars,
        total_chars,
        words: selected_metrics.words,
        total_words: entry.metrics.words,
        truncated: start_char > 0 || end_char < total_chars,
        content,
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
            "\n{}. {} | {} | world={} | chars={} | words={}",
            index + 1,
            entry.ref_id,
            display_label(entry),
            entry.world,
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
            "\n\n{} | {} | world={} | chars {}-{} of {} | words {} of {}",
            entry.ref_id,
            display_label_rendered(entry),
            entry.world,
            entry.start_char,
            entry.end_char,
            entry.total_chars,
            entry.words,
            entry.total_words
        ));
        if let Some(position) = &entry.position {
            content.push_str(&format!(" | position={position}"));
        }
        if entry.truncated {
            content.push_str(" | truncated");
        }
        content.push('\n');
        content.push_str(&entry.content);
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
        range: TextRangeMetricsPayload::new(
            TextMetrics {
                chars: entry.chars,
                words: entry.words,
            },
            TextMetrics {
                chars: entry.total_chars,
                words: entry.total_words,
            },
            entry.start_char,
            entry.end_char,
        ),
        content: entry.content.as_str(),
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

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end - start).collect()
}

fn invalid_activation_snapshot(message: impl Into<String>) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.invalid_worldinfo_activation_snapshot: {}",
        message.into()
    ))
}
