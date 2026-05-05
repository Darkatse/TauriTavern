use serde_json::{Map, Value, json};

use super::{
    MAX_WORLDINFO_ENTRIES_PER_READ, MAX_WORLDINFO_ENTRY_RANGE_CHARS,
    MAX_WORLDINFO_FULL_ENTRY_CHARS, MAX_WORLDINFO_TOTAL_READ_CHARS,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::common::{object_args, tool_error};
use crate::application::services::agent_tools::dispatcher::AgentToolEffect;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, WorkspacePath};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

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
    total_chars: usize,
    truncated: bool,
    content: String,
    ref_id: String,
}

pub(in crate::application::services::agent_tools) async fn read_activated(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
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

    let snapshot_path = WorkspacePath::parse("input/prompt_snapshot.json")?;
    let snapshot_file = workspace_repository
        .read_text(run_id, &snapshot_path)
        .await
        .map_err(ApplicationError::from)?;
    let snapshot: Value = serde_json::from_str(&snapshot_file.text).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "agent.invalid_prompt_snapshot_file: failed to parse prompt snapshot JSON: {error}"
        ))
    })?;

    let Some(batch) = snapshot.get("worldInfoActivation") else {
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
        structured: json!({
            "mode": "index",
            "timestampMs": batch.get("timestampMs").and_then(Value::as_i64),
            "trigger": batch.get("trigger").and_then(Value::as_str),
            "totalEntries": entries.len(),
            "entries": entries.iter().map(index_entry).collect::<Vec<_>>(),
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
        total_returned_chars += item.content.chars().count();
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
        structured: json!({
            "mode": "content",
            "timestampMs": batch.get("timestampMs").and_then(Value::as_i64),
            "trigger": batch.get("trigger").and_then(Value::as_str),
            "totalEntries": entries.len(),
            "entries": rendered.iter().map(content_entry).collect::<Vec<_>>(),
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
    let total_chars = entry.content.chars().count();
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

    Ok(RenderedEntry {
        world: entry.world.clone(),
        uid: entry.uid.clone(),
        display_name: entry.display_name.clone(),
        constant: entry.constant,
        position: entry.position.clone(),
        start_char,
        end_char,
        total_chars,
        truncated: end_char < total_chars,
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
            "\n{}. {} | {} | world={} | chars={}",
            index + 1,
            entry.ref_id,
            display_label(entry),
            entry.world,
            entry.content.chars().count()
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
            "\n\n{} | {} | world={} | chars {}-{} of {}",
            entry.ref_id,
            display_label_rendered(entry),
            entry.world,
            entry.start_char,
            entry.end_char,
            entry.total_chars
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

fn index_entry(entry: &ActivatedEntry) -> Value {
    json!({
        "world": entry.world.as_str(),
        "uid": entry.uid.as_str(),
        "displayName": entry.display_name.as_deref(),
        "constant": entry.constant,
        "position": entry.position.as_deref(),
        "totalChars": entry.content.chars().count(),
        "ref": entry.ref_id.as_str(),
    })
}

fn content_entry(entry: &RenderedEntry) -> Value {
    json!({
        "world": entry.world.as_str(),
        "uid": entry.uid.as_str(),
        "displayName": entry.display_name.as_deref(),
        "constant": entry.constant,
        "position": entry.position.as_deref(),
        "startChar": entry.start_char,
        "endChar": entry.end_char,
        "totalChars": entry.total_chars,
        "truncated": entry.truncated,
        "content": entry.content.as_str(),
        "ref": entry.ref_id.as_str(),
    })
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
