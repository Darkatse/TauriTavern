use serde_json::{Map, Value};
use tt_contracts::observability::{
    MessageViewDto, ParameterViewDto, PartViewDto, RequestKindDto, RequestViewDto, ResponseViewDto,
    ToolViewDto, UsageViewDto,
};
use tt_ports::repositories::chat_completion_repository::{
    NATIVE_CLAUDE, NATIVE_GEMINI, NATIVE_GEMINI_INTERACTIONS, NATIVE_OPENAI_RESPONSES,
};

use super::reasoning::{
    collect_native_state_parts, collect_visible_reasoning_texts, collect_visible_reasoning_value,
    value_encoded_len,
};
use super::text_completion::{project_text_candidates, project_text_request};
use super::wire::{data_url_media, parse_media_marker};

const SYSTEM_FIELD_CANDIDATES: [&str; 4] = [
    "system",
    "systemInstruction",
    "system_instruction",
    "instructions",
];

const TOOL_CHOICE_FIELD_CANDIDATES: [&str; 3] = ["tool_choice", "toolChoice", "toolConfig"];

/// The role of a response candidate whose wire form has none: Bedrock text frames and
/// role-less chunk streams.
const RESPONSE_ROLE: &str = "assistant";

pub fn project_request_view(kind: RequestKindDto, payload: &Value) -> RequestViewDto {
    if kind == RequestKindDto::Text {
        return project_text_request(payload);
    }

    let Some(root) = payload.as_object() else {
        return RequestViewDto {
            kind: RequestKindDto::Chat,
            http: None,
            system_field: None,
            system_path: None,
            system: Vec::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            tool_choice: None,
            parameters: Vec::new(),
            prompt: None,
        };
    };

    let container_key = if root.contains_key("contents") {
        Some("contents")
    } else if root.contains_key("input") {
        Some("input")
    } else if root.contains_key("messages") {
        Some("messages")
    } else {
        None
    };

    let system_entry = SYSTEM_FIELD_CANDIDATES.into_iter().find_map(|key| {
        let value = root.get(key).filter(|value| !value.is_null())?;
        let path = pointer_push("", key);
        let parts = project_system_value(value, &path);
        Some((key.to_string(), path, parts))
    });
    let (system_field, system_path, system) = match system_entry {
        Some((field, path, parts)) => (Some(field), Some(path), parts),
        None => (None, None, Vec::new()),
    };

    let has_tools_array = root.get("tools").is_some_and(Value::is_array);
    let tools = root
        .get("tools")
        .and_then(Value::as_array)
        .map(|items| project_tools(items, "/tools"))
        .unwrap_or_default();

    let tool_choice_key = TOOL_CHOICE_FIELD_CANDIDATES
        .into_iter()
        .find(|key| root.get(*key).is_some_and(|value| !value.is_null()));
    let tool_choice = tool_choice_key.and_then(|key| root.get(key).cloned());

    let messages = match container_key {
        Some("contents") => project_gemini_contents(root.get("contents"), "/contents"),
        Some("input") => project_input_items(root.get("input"), "/input"),
        Some("messages") => project_messages_array(root.get("messages"), "/messages"),
        _ => Vec::new(),
    };

    let mut parameters = Vec::new();
    for (key, value) in root {
        if container_key == Some(key.as_str())
            || system_field.as_deref() == Some(key.as_str())
            || (has_tools_array && key == "tools")
            || tool_choice_key == Some(key.as_str())
        {
            continue;
        }
        parameters.push(ParameterViewDto {
            key: key.clone(),
            path: pointer_push("", key),
            value: value.clone(),
        });
    }

    RequestViewDto {
        kind: RequestKindDto::Chat,
        http: None,
        system_field,
        system_path,
        system,
        messages,
        tools,
        tool_choice,
        parameters,
        prompt: None,
    }
}

pub fn project_response_view(
    kind: RequestKindDto,
    document: &Value,
    base_path: Option<&str>,
) -> ResponseViewDto {
    let (candidates, finish_reason) = match kind {
        RequestKindDto::Chat => project_chat_candidates(document, base_path),
        RequestKindDto::Text => project_text_candidates(document, base_path),
    };
    ResponseViewDto {
        candidates,
        finish_reason,
        usage: extract_usage_from_response(document),
        error: extract_response_error(document),
    }
}

fn project_chat_candidates(
    document: &Value,
    base_path: Option<&str>,
) -> (Vec<MessageViewDto>, Option<String>) {
    if let Some(choices) = document.get("choices").and_then(Value::as_array) {
        let choices_path = base_path.map(|base| pointer_push(base, "choices"));
        let mut candidates = Vec::with_capacity(choices.len());
        for (index, choice) in choices.iter().enumerate() {
            let choice_path = choices_path
                .as_deref()
                .map(|base| pointer_index(base, index));
            if let Some(message) = choice.get("message").and_then(Value::as_object) {
                let message_path = choice_path
                    .as_deref()
                    .map(|base| pointer_push(base, "message"));
                candidates.push(project_response_message(message, message_path.as_deref()));
            } else {
                candidates.push(MessageViewDto {
                    role: RESPONSE_ROLE.to_string(),
                    name: None,
                    path: choice_path.clone(),
                    parts: vec![PartViewDto::Other {
                        label: "choice".to_string(),
                        value: choice.clone(),
                        path: choice_path,
                    }],
                });
            }
        }
        let finish_reason = choices.iter().find_map(|choice| {
            choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
        return (candidates, finish_reason);
    }

    // Cohere's own non-streamed response keeps the message at the top level.
    if let Some(message) = document.get("message").and_then(Value::as_object) {
        let message_path = base_path.map(|base| pointer_push(base, "message"));
        let finish_reason = document
            .get("finish_reason")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        return (
            vec![project_response_message(message, message_path.as_deref())],
            finish_reason,
        );
    }

    (
        vec![MessageViewDto {
            role: RESPONSE_ROLE.to_string(),
            name: None,
            path: base_path.map(str::to_string),
            parts: vec![PartViewDto::Other {
                label: "response".to_string(),
                value: document.clone(),
                path: base_path.map(str::to_string),
            }],
        }],
        None,
    )
}

/// A normalized response keeps the provider's own content under `native`; the OpenAI-shaped
/// fields beside it are a lossy projection of the same content, so exactly one of the two
/// is shown.
fn project_response_message(
    message: &Map<String, Value>,
    base_path: Option<&str>,
) -> MessageViewDto {
    let Some(native) = message.get("native").and_then(Value::as_object) else {
        return project_message_object(message, base_path, RESPONSE_ROLE);
    };

    let mut role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or(RESPONSE_ROLE)
        .to_string();
    let mut parts = Vec::new();
    for (provider, value) in native {
        let provider_path =
            base_path.map(|base| pointer_push(&pointer_push(base, "native"), provider));
        match provider.as_str() {
            NATIVE_CLAUDE => {
                let content_path = provider_path
                    .as_deref()
                    .map(|base| pointer_push(base, "content"));
                parts.extend(project_content_value(
                    value.get("content"),
                    content_path.as_deref(),
                ));
            }
            NATIVE_GEMINI => {
                if let Some(content) = value.get("content").and_then(Value::as_object) {
                    if let Some(content_role) = content.get("role").and_then(Value::as_str) {
                        role = content_role.to_string();
                    }
                    if let Some(gemini_parts) = content.get("parts").and_then(Value::as_array) {
                        let parts_path = provider_path
                            .as_deref()
                            .map(|base| pointer_push(&pointer_push(base, "content"), "parts"));
                        parts.extend(project_gemini_parts(gemini_parts, parts_path.as_deref()));
                    }
                }
            }
            NATIVE_OPENAI_RESPONSES => {
                parts.extend(native_item_parts(value, "output", provider_path.as_deref()));
            }
            NATIVE_GEMINI_INTERACTIONS => {
                parts.extend(native_item_parts(value, "steps", provider_path.as_deref()));
            }
            _ => parts.push(PartViewDto::Other {
                label: provider.clone(),
                value: value.clone(),
                path: provider_path,
            }),
        }
    }
    MessageViewDto {
        role,
        name: None,
        path: base_path.map(str::to_string),
        parts,
    }
}

fn native_item_parts(native: &Value, key: &str, provider_path: Option<&str>) -> Vec<PartViewDto> {
    let Some(items) = native.get(key).and_then(Value::as_array) else {
        return Vec::new();
    };
    let items_path = provider_path.map(|base| pointer_push(base, key));
    project_responses_output_items(items, items_path.as_deref())
        .into_iter()
        .flat_map(|item| item.parts)
        .collect()
}

pub(super) fn pointer_push(base: &str, segment: &str) -> String {
    let mut out = String::with_capacity(base.len() + 1 + segment.len());
    out.push_str(base);
    out.push('/');
    for ch in segment.chars() {
        match ch {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            _ => out.push(ch),
        }
    }
    out
}

pub(super) fn pointer_index(base: &str, index: usize) -> String {
    format!("{base}/{index}")
}

fn parse_tool_arguments(raw: Option<&Value>) -> (Value, Option<String>) {
    let Some(raw) = raw else {
        return (Value::Object(Map::new()), None);
    };
    match raw {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(parsed) => (parsed, None),
            Err(_) => (Value::Null, Some(text.clone())),
        },
        other => (other.clone(), None),
    }
}

fn extract_usage_from_response(response: &Value) -> Option<UsageViewDto> {
    if let Some(usage) = response.get("usage").and_then(Value::as_object) {
        let parsed = parse_usage_object(usage);
        if !parsed.is_empty() {
            return Some(parsed);
        }
    }
    if let Some(usage) = response.get("usageMetadata").and_then(Value::as_object) {
        let parsed = parse_gemini_usage_metadata(usage);
        if !parsed.is_empty() {
            return Some(parsed);
        }
    }
    None
}

fn parse_usage_object(usage: &Map<String, Value>) -> UsageViewDto {
    let count = |key: &str| usage.get(key).and_then(Value::as_u64);
    let detail = |object: &str, key: &str| {
        usage
            .get(object)
            .and_then(|details| details.get(key))
            .and_then(Value::as_u64)
    };
    let cohere = |key: &str| detail("tokens", key).or_else(|| detail("billed_units", key));
    // Anthropic reports cached input beside `input_tokens`; the adapter's `map_claude_usage`
    // (tt-adapter-provider-http `normalizers.rs`) adds them up the same way.
    let anthropic_input = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .into_iter()
    .filter_map(count)
    .reduce(|sum, tokens| sum + tokens);

    UsageViewDto {
        input: count("prompt_tokens")
            .or(anthropic_input)
            .or_else(|| cohere("input_tokens")),
        cache_read: detail("prompt_tokens_details", "cached_tokens")
            .or_else(|| count("cache_read_input_tokens"))
            .or_else(|| count("cached_tokens")),
        output: count("completion_tokens")
            .or_else(|| count("output_tokens"))
            .or_else(|| cohere("output_tokens")),
        reasoning: detail("completion_tokens_details", "reasoning_tokens"),
    }
}

fn parse_gemini_usage_metadata(usage: &Map<String, Value>) -> UsageViewDto {
    UsageViewDto {
        input: usage.get("promptTokenCount").and_then(Value::as_u64),
        cache_read: usage.get("cachedContentTokenCount").and_then(Value::as_u64),
        output: usage.get("candidatesTokenCount").and_then(Value::as_u64),
        reasoning: usage.get("thoughtsTokenCount").and_then(Value::as_u64),
    }
}

fn project_system_value(value: &Value, base_path: &str) -> Vec<PartViewDto> {
    if let Some(object) = value.as_object()
        && let Some(parts) = object.get("parts").and_then(Value::as_array)
    {
        let parts_path = pointer_push(base_path, "parts");
        return project_gemini_parts(parts, Some(&parts_path));
    }
    project_content_value(Some(value), Some(base_path))
}

fn project_tools(tools: &[Value], base_path: &str) -> Vec<ToolViewDto> {
    let mut out = Vec::new();
    for (index, tool) in tools.iter().enumerate() {
        let tool_path = pointer_index(base_path, index);
        let Some(object) = tool.as_object() else {
            out.push(ToolViewDto {
                name: "<invalid tool>".to_string(),
                description: None,
                parameters: Some(tool.clone()),
                path: tool_path,
            });
            continue;
        };

        if let Some(function) = object.get("function").and_then(Value::as_object) {
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("<missing name>")
                .to_string();
            let description = non_empty_string_field(function, "description");
            let parameters = function
                .get("parameters")
                .or_else(|| function.get("input_schema"))
                .cloned();
            out.push(ToolViewDto {
                name,
                description,
                parameters,
                path: tool_path,
            });
            continue;
        }

        if let Some((key, declarations)) = ["functionDeclarations", "function_declarations"]
            .into_iter()
            .find_map(|key| Some((key, object.get(key)?.as_array()?)))
        {
            let decls_base = pointer_push(&tool_path, key);
            for (decl_index, declaration) in declarations.iter().enumerate() {
                let decl_path = pointer_index(&decls_base, decl_index);
                let Some(decl_obj) = declaration.as_object() else {
                    out.push(ToolViewDto {
                        name: "<invalid declaration>".to_string(),
                        description: None,
                        parameters: Some(declaration.clone()),
                        path: decl_path,
                    });
                    continue;
                };
                let name = decl_obj
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing name>")
                    .to_string();
                let description = non_empty_string_field(decl_obj, "description");
                let parameters = decl_obj
                    .get("parameters")
                    .or_else(|| decl_obj.get("parametersJsonSchema"))
                    .cloned();
                out.push(ToolViewDto {
                    name,
                    description,
                    parameters,
                    path: decl_path,
                });
            }
            continue;
        }

        if let Some(name) = object.get("name").and_then(Value::as_str) {
            let description = non_empty_string_field(object, "description");
            let parameters = object
                .get("parameters")
                .or_else(|| object.get("input_schema"))
                .cloned();
            out.push(ToolViewDto {
                name: name.to_string(),
                description,
                parameters,
                path: tool_path,
            });
            continue;
        }

        if let Some(kind) = object.get("type").and_then(Value::as_str) {
            let mut rest = object.clone();
            rest.remove("type");
            out.push(ToolViewDto {
                name: kind.to_string(),
                description: None,
                parameters: (!rest.is_empty()).then_some(Value::Object(rest)),
                path: tool_path,
            });
            continue;
        }

        if object.len() == 1
            && let Some((key, value)) = object.iter().next()
        {
            let parameters = match value {
                Value::Null => None,
                Value::Object(map) if map.is_empty() => None,
                other => Some(other.clone()),
            };
            out.push(ToolViewDto {
                name: key.clone(),
                description: None,
                parameters,
                path: tool_path,
            });
            continue;
        }

        out.push(ToolViewDto {
            name: "<unknown tool>".to_string(),
            description: None,
            parameters: Some(tool.clone()),
            path: tool_path,
        });
    }
    out
}

fn project_messages_array(messages: Option<&Value>, base_path: &str) -> Vec<MessageViewDto> {
    let Some(items) = messages.and_then(Value::as_array) else {
        return Vec::new();
    };

    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let item_path = pointer_index(base_path, index);
            let Some(object) = item.as_object() else {
                return MessageViewDto {
                    role: "unknown".to_string(),
                    name: None,
                    path: Some(item_path.clone()),
                    parts: vec![PartViewDto::Other {
                        label: "invalid_message".to_string(),
                        value: item.clone(),
                        path: Some(item_path),
                    }],
                };
            };
            project_message_object(object, Some(&item_path), "unknown")
        })
        .collect()
}

fn project_message_object(
    message: &Map<String, Value>,
    base_path: Option<&str>,
    missing_role: &str,
) -> MessageViewDto {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or(missing_role)
        .to_string();
    let name = non_empty_string_field(message, "name");
    let path = base_path.map(str::to_string);

    if role == "tool" {
        let call_id = message
            .get("tool_call_id")
            .or_else(|| message.get("tool_use_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let is_error = message.get("is_error").and_then(Value::as_bool);
        let content_path = base_path.map(|base| pointer_push(base, "content"));
        let result_parts = project_content_value(message.get("content"), content_path.as_deref());
        return MessageViewDto {
            role,
            name: None,
            path: path.clone(),
            parts: vec![PartViewDto::ToolResult {
                call_id,
                name,
                is_error,
                parts: result_parts,
                path,
            }],
        };
    }

    let mut parts = Vec::new();

    append_message_reasoning(&mut parts, message, base_path);

    if let Some(tool_plan) = non_empty_string_field(message, "tool_plan") {
        parts.push(PartViewDto::Reasoning {
            text: tool_plan,
            path: base_path.map(|base| pointer_push(base, "tool_plan")),
        });
    }

    parts.extend(collect_native_state_parts(message, base_path));

    let content_path = base_path.map(|base| pointer_push(base, "content"));
    parts.extend(project_content_value(
        message.get("content"),
        content_path.as_deref(),
    ));

    if let Some(tool_calls) = message.get("tool_calls") {
        let calls_path = base_path.map(|base| pointer_push(base, "tool_calls"));
        append_openai_tool_calls(&mut parts, tool_calls, calls_path.as_deref());
    }

    MessageViewDto {
        role,
        name,
        path,
        parts,
    }
}

/// The visible reasoning of an OpenAI-shaped message comes from one source. The adapter's
/// `normalize_message_reasoning` (tt-adapter-provider-http `openai.rs`) copies it into
/// `reasoning_content` and keeps the original fields, so reading more than one repeats it.
fn append_message_reasoning(
    parts: &mut Vec<PartViewDto>,
    message: &Map<String, Value>,
    base_path: Option<&str>,
) {
    let details = message
        .get("reasoning_details")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let details_path = base_path.map(|base| pointer_push(base, "reasoning_details"));

    let reasoning = ["reasoning_content", "reasoning"]
        .into_iter()
        .find_map(|key| {
            Some((
                non_empty_string_field(message, key)?,
                base_path.map(|base| pointer_push(base, key)),
            ))
        })
        .or_else(|| {
            let text: String = details
                .iter()
                .filter_map(|detail| match detail.get("type").and_then(Value::as_str) {
                    Some("reasoning.text") => detail.get("text").and_then(Value::as_str),
                    Some("reasoning.summary") => detail.get("summary").and_then(Value::as_str),
                    _ => None,
                })
                .collect();
            (!text.is_empty()).then(|| (text, details_path.clone()))
        });
    if let Some((text, path)) = reasoning {
        parts.push(PartViewDto::Reasoning { text, path });
    }

    for (index, detail) in details.iter().enumerate() {
        if detail.get("type").and_then(Value::as_str) != Some("reasoning.encrypted") {
            continue;
        }
        let length = detail
            .get("data")
            .map(value_encoded_len)
            .unwrap_or_default();
        if length > 0 {
            parts.push(PartViewDto::NativeState {
                kind: "reasoning.encrypted".to_string(),
                length,
                path: details_path
                    .as_deref()
                    .map(|base| pointer_index(base, index)),
            });
        }
    }
}

/// Empty text is no part unless it carries a cache breakpoint; whitespace is text.
fn text_part(text: &str, cache_breakpoint: bool, path: Option<String>) -> Option<PartViewDto> {
    (!text.is_empty() || cache_breakpoint).then(|| PartViewDto::Text {
        text: text.to_string(),
        cache_breakpoint,
        path,
    })
}

fn project_content_value(content: Option<&Value>, base_path: Option<&str>) -> Vec<PartViewDto> {
    let Some(content) = content else {
        return Vec::new();
    };
    match content {
        Value::Null => Vec::new(),
        Value::String(text) => text_part(text, false, base_path.map(str::to_string))
            .into_iter()
            .collect(),
        Value::Array(items) => {
            let mut out = Vec::new();
            for (index, item) in items.iter().enumerate() {
                let item_path = base_path.map(|base| pointer_index(base, index));
                project_content_item(&mut out, item, item_path.as_deref());
            }
            out
        }
        other => text_part(&other.to_string(), false, base_path.map(str::to_string))
            .into_iter()
            .collect(),
    }
}

fn project_content_item(out: &mut Vec<PartViewDto>, item: &Value, item_path: Option<&str>) {
    if let Some(text) = item.as_str() {
        out.extend(text_part(text, false, item_path.map(str::to_string)));
        return;
    }

    let Some(object) = item.as_object() else {
        out.push(PartViewDto::Other {
            label: "content".to_string(),
            value: item.clone(),
            path: item_path.map(str::to_string),
        });
        return;
    };

    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("content");

    match item_type {
        "text" | "input_text" | "output_text" => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                out.extend(text_part(
                    text,
                    has_cache_control(object),
                    item_path.map(str::to_string),
                ));
            } else {
                out.push(PartViewDto::Other {
                    label: item_type.to_string(),
                    value: item.clone(),
                    path: item_path.map(str::to_string),
                });
            }
        }
        "reasoning" | "thinking" | "thought" => {
            append_reasoning_object_parts(out, object, item_path);
        }
        "redacted_thinking" => {
            let length = object
                .get("data")
                .map(value_encoded_len)
                .unwrap_or_default();
            out.push(PartViewDto::NativeState {
                kind: "redacted_thinking".to_string(),
                length,
                path: item_path.map(str::to_string),
            });
        }
        "tool_use" => {
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("<missing name>")
                .to_string();
            let (arguments, arguments_text) = parse_tool_arguments(object.get("input"));
            out.push(PartViewDto::ToolCall {
                id,
                name,
                arguments,
                arguments_text,
                path: item_path.map(str::to_string),
            });
            out.extend(collect_native_state_parts(object, item_path));
        }
        "tool_result" => {
            let call_id = object
                .get("tool_use_id")
                .or_else(|| object.get("call_id"))
                .or_else(|| object.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let name = non_empty_string_field(object, "name");
            let is_error = object.get("is_error").and_then(Value::as_bool);
            let content_path = item_path.map(|base| pointer_push(base, "content"));
            let parts = project_content_value(object.get("content"), content_path.as_deref());
            out.push(PartViewDto::ToolResult {
                call_id,
                name,
                is_error,
                parts,
                path: item_path.map(str::to_string),
            });
        }
        "function_call" => append_function_call_parts(out, object, item_path),
        "function_result" | "function_call_output" => {
            append_function_result_part(out, object, item_path);
        }
        "image_url" | "input_image" | "audio_url" | "input_audio" | "video_url" | "image"
        | "audio" | "video" | "document" | "file" | "input_file" => {
            if let Some(media) = project_media_object(item_type, object, item_path) {
                out.push(media);
            } else {
                out.push(PartViewDto::Other {
                    label: item_type.to_string(),
                    value: item.clone(),
                    path: item_path.map(str::to_string),
                });
            }
        }
        _ => {
            out.push(PartViewDto::Other {
                label: item_type.to_string(),
                value: item.clone(),
                path: item_path.map(str::to_string),
            });
        }
    }
}

fn append_reasoning_object_parts(
    out: &mut Vec<PartViewDto>,
    object: &Map<String, Value>,
    base_path: Option<&str>,
) {
    let texts = collect_visible_reasoning_texts(object);
    if !texts.is_empty() {
        out.push(PartViewDto::Reasoning {
            text: texts.join("\n\n"),
            path: base_path.map(str::to_string),
        });
    }
    out.extend(collect_native_state_parts(object, base_path));
}

fn append_function_call_parts(
    out: &mut Vec<PartViewDto>,
    object: &Map<String, Value>,
    base_path: Option<&str>,
) {
    let id = object
        .get("call_id")
        .or_else(|| object.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<missing name>")
        .to_string();
    let raw_args = object
        .get("arguments")
        .or_else(|| object.get("args"))
        .or_else(|| object.get("input"));
    let (arguments, arguments_text) = parse_tool_arguments(raw_args);

    out.push(PartViewDto::ToolCall {
        id,
        name,
        arguments,
        arguments_text,
        path: base_path.map(str::to_string),
    });
    out.extend(collect_native_state_parts(object, base_path));
}

fn append_function_result_part(
    out: &mut Vec<PartViewDto>,
    object: &Map<String, Value>,
    base_path: Option<&str>,
) {
    let call_id = object
        .get("call_id")
        .or_else(|| object.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = non_empty_string_field(object, "name");
    let is_error = object.get("is_error").and_then(Value::as_bool);

    let (field_name, raw_result) = if let Some(val) = object.get("result") {
        ("result", Some(val))
    } else if let Some(val) = object.get("output") {
        ("output", Some(val))
    } else if let Some(val) = object.get("response") {
        ("response", Some(val))
    } else {
        ("output", None)
    };
    let result_path = base_path.map(|base| pointer_push(base, field_name));
    let parts = project_content_value(raw_result, result_path.as_deref());

    out.push(PartViewDto::ToolResult {
        call_id,
        name,
        is_error,
        parts,
        path: base_path.map(str::to_string),
    });
}

fn append_openai_tool_calls(
    out: &mut Vec<PartViewDto>,
    tool_calls: &Value,
    base_path: Option<&str>,
) {
    let Some(calls) = tool_calls.as_array() else {
        out.push(PartViewDto::Other {
            label: "invalid_tool_calls".to_string(),
            value: tool_calls.clone(),
            path: base_path.map(str::to_string),
        });
        return;
    };

    for (index, call) in calls.iter().enumerate() {
        let call_path = base_path.map(|base| pointer_index(base, index));
        let Some(object) = call.as_object() else {
            out.push(PartViewDto::Other {
                label: "invalid_tool_call".to_string(),
                value: call.clone(),
                path: call_path,
            });
            continue;
        };

        let id = object
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let function = object.get("function").and_then(Value::as_object);
        let name = function
            .and_then(|fn_obj| fn_obj.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("<missing name>")
            .to_string();
        let (arguments, arguments_text) =
            parse_tool_arguments(function.and_then(|fn_obj| fn_obj.get("arguments")));

        out.push(PartViewDto::ToolCall {
            id,
            name,
            arguments,
            arguments_text,
            path: call_path.clone(),
        });
        out.extend(collect_native_state_parts(object, call_path.as_deref()));
    }
}

fn project_input_items(input: Option<&Value>, base_path: &str) -> Vec<MessageViewDto> {
    let Some(input) = input else {
        return Vec::new();
    };
    match input {
        Value::String(text) => vec![MessageViewDto {
            role: "user".to_string(),
            name: None,
            path: Some(base_path.to_string()),
            parts: text_part(text, false, Some(base_path.to_string()))
                .into_iter()
                .collect(),
        }],
        Value::Array(items) => project_responses_output_items(items, Some(base_path)),
        other => vec![MessageViewDto {
            role: "user".to_string(),
            name: None,
            path: Some(base_path.to_string()),
            parts: vec![PartViewDto::Other {
                label: "input".to_string(),
                value: other.clone(),
                path: Some(base_path.to_string()),
            }],
        }],
    }
}

fn project_responses_output_items(items: &[Value], base_path: Option<&str>) -> Vec<MessageViewDto> {
    let mut messages = Vec::new();

    for (index, item) in items.iter().enumerate() {
        let item_path = base_path.map(|base| pointer_index(base, index));
        let Some(object) = item.as_object() else {
            messages.push(MessageViewDto {
                role: "unknown".to_string(),
                name: None,
                path: item_path.clone(),
                parts: vec![PartViewDto::Other {
                    label: "invalid_item".to_string(),
                    value: item.clone(),
                    path: item_path,
                }],
            });
            continue;
        };

        if object.get("role").and_then(Value::as_str).is_some() {
            messages.push(project_message_object(
                object,
                item_path.as_deref(),
                "unknown",
            ));
            continue;
        }

        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        match item_type {
            "user_input" | "model_output" => {
                let role = if item_type == "user_input" {
                    "user"
                } else {
                    "model"
                };
                let content_path = item_path
                    .as_deref()
                    .map(|base| pointer_push(base, "content"));
                let mut parts =
                    project_content_value(object.get("content"), content_path.as_deref());
                parts.extend(collect_native_state_parts(object, item_path.as_deref()));
                messages.push(MessageViewDto {
                    role: role.to_string(),
                    name: None,
                    path: item_path,
                    parts,
                });
            }
            "message" => {
                let mut parts = Vec::new();
                if let Some(reasoning) = object.get("reasoning_content") {
                    let r_path = item_path
                        .as_deref()
                        .map(|base| pointer_push(base, "reasoning_content"));
                    for text in collect_visible_reasoning_value(reasoning) {
                        parts.push(PartViewDto::Reasoning {
                            text,
                            path: r_path.clone(),
                        });
                    }
                }
                let content_path = item_path
                    .as_deref()
                    .map(|base| pointer_push(base, "content"));
                parts.extend(project_content_value(
                    object.get("content"),
                    content_path.as_deref(),
                ));
                messages.push(MessageViewDto {
                    role: "assistant".to_string(),
                    name: None,
                    path: item_path,
                    parts,
                });
            }
            "function_call" => {
                let mut parts = Vec::new();
                append_function_call_parts(&mut parts, object, item_path.as_deref());
                messages.push(MessageViewDto {
                    role: "assistant".to_string(),
                    name: None,
                    path: item_path,
                    parts,
                });
            }
            "function_call_output" | "function_result" => {
                let mut parts = Vec::new();
                append_function_result_part(&mut parts, object, item_path.as_deref());
                messages.push(MessageViewDto {
                    role: "tool".to_string(),
                    name: None,
                    path: item_path,
                    parts,
                });
            }
            "reasoning" | "thinking" | "thought" => {
                let mut parts = Vec::new();
                append_reasoning_object_parts(&mut parts, object, item_path.as_deref());
                messages.push(MessageViewDto {
                    role: "assistant".to_string(),
                    name: None,
                    path: item_path,
                    parts,
                });
            }
            _ => {
                let mut parts = collect_native_state_parts(object, item_path.as_deref());
                if parts.is_empty() {
                    parts.push(PartViewDto::Other {
                        label: item_type.to_string(),
                        value: item.clone(),
                        path: item_path.clone(),
                    });
                }
                messages.push(MessageViewDto {
                    role: "assistant".to_string(),
                    name: None,
                    path: item_path,
                    parts,
                });
            }
        }
    }

    messages
}

fn project_gemini_contents(contents: Option<&Value>, base_path: &str) -> Vec<MessageViewDto> {
    let Some(items) = contents.and_then(Value::as_array) else {
        return Vec::new();
    };

    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let content_path = pointer_index(base_path, index);
            let Some(object) = item.as_object() else {
                return MessageViewDto {
                    role: "unknown".to_string(),
                    name: None,
                    path: Some(content_path.clone()),
                    parts: vec![PartViewDto::Other {
                        label: "invalid_content".to_string(),
                        value: item.clone(),
                        path: Some(content_path),
                    }],
                };
            };
            let role = object
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let parts = match object.get("parts").and_then(Value::as_array) {
                Some(parts_arr) => {
                    let parts_path = pointer_push(&content_path, "parts");
                    project_gemini_parts(parts_arr, Some(&parts_path))
                }
                None => Vec::new(),
            };
            MessageViewDto {
                role,
                name: None,
                path: Some(content_path),
                parts,
            }
        })
        .collect()
}

fn project_gemini_parts(parts: &[Value], base_path: Option<&str>) -> Vec<PartViewDto> {
    let mut out = Vec::new();

    for (index, part) in parts.iter().enumerate() {
        let part_path = base_path.map(|base| pointer_index(base, index));
        let Some(object) = part.as_object() else {
            out.push(PartViewDto::Other {
                label: "part".to_string(),
                value: part.clone(),
                path: part_path,
            });
            continue;
        };

        let is_thought = object
            .get("thought")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if is_thought {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    out.push(PartViewDto::Reasoning {
                        text: text.to_string(),
                        path: part_path.clone(),
                    });
                }
                out.extend(collect_native_state_parts(object, part_path.as_deref()));
            } else {
                append_reasoning_object_parts(&mut out, object, part_path.as_deref());
            }
            continue;
        }

        if let Some(text) = object.get("text").and_then(Value::as_str) {
            out.extend(text_part(
                text,
                has_cache_control(object),
                part_path.clone(),
            ));
            out.extend(collect_native_state_parts(object, part_path.as_deref()));
            continue;
        }

        if let Some(function_call) = object.get("functionCall").and_then(Value::as_object) {
            let id = function_call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let name = function_call
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("<missing name>")
                .to_string();
            let (arguments, arguments_text) = parse_tool_arguments(function_call.get("args"));
            out.push(PartViewDto::ToolCall {
                id,
                name,
                arguments,
                arguments_text,
                path: part_path.clone(),
            });
            out.extend(collect_native_state_parts(object, part_path.as_deref()));
            continue;
        }

        if let Some(function_response) = object.get("functionResponse").and_then(Value::as_object) {
            let call_id = function_response
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let name = non_empty_string_field(function_response, "name");
            let response_path = part_path
                .as_deref()
                .map(|base| format!("{base}/functionResponse/response"));
            let result_parts = project_gemini_response_payload(
                function_response.get("response"),
                response_path.as_deref(),
            );
            out.push(PartViewDto::ToolResult {
                call_id,
                name,
                is_error: None,
                parts: result_parts,
                path: part_path,
            });
            continue;
        }

        if let Some(inline_data) = object
            .get("inlineData")
            .or_else(|| object.get("inline_data"))
            .and_then(Value::as_object)
            && let Some(media) =
                media_from_data_object(inline_data, "application/octet-stream", part_path.clone())
        {
            out.push(media);
            continue;
        }

        if let Some(file_data) = object
            .get("fileData")
            .or_else(|| object.get("file_data"))
            .and_then(Value::as_object)
            && let Some(media) =
                media_from_data_object(file_data, "application/octet-stream", part_path.clone())
        {
            out.push(media);
            continue;
        }

        let native_states = collect_native_state_parts(object, part_path.as_deref());
        if !native_states.is_empty() {
            out.extend(native_states);
            continue;
        }

        out.push(PartViewDto::Other {
            label: "part".to_string(),
            value: part.clone(),
            path: part_path,
        });
    }

    out
}

fn project_gemini_response_payload(
    response: Option<&Value>,
    base_path: Option<&str>,
) -> Vec<PartViewDto> {
    let Some(response) = response else {
        return Vec::new();
    };
    if let Some(obj) = response.as_object()
        && obj.len() == 1
        && let Some(Value::String(content)) = obj.get("content")
    {
        return text_part(
            content,
            false,
            base_path.map(|base| pointer_push(base, "content")),
        )
        .into_iter()
        .collect();
    }
    project_content_value(Some(response), base_path)
}

fn project_media_object(
    item_type: &str,
    object: &Map<String, Value>,
    base_path: Option<&str>,
) -> Option<PartViewDto> {
    let path = base_path.map(str::to_string);
    let fallback_mime = fallback_mime_for_type(item_type);

    for url_field in ["image_url", "audio_url", "video_url"] {
        if let Some(target) = object.get(url_field) {
            if let Some(url) = target.as_str() {
                return Some(media_from_url(url, fallback_mime, path));
            }
            if let Some(nested) = target.as_object() {
                return media_from_data_object(nested, fallback_mime, path);
            }
        }
    }

    if let Some(audio) = object.get("input_audio").and_then(Value::as_object) {
        return media_from_data_object(audio, "audio/*", path);
    }

    if let Some(source) = object.get("source").and_then(Value::as_object) {
        return media_from_data_object(source, fallback_mime, path);
    }

    media_from_data_object(object, fallback_mime, path)
}

fn media_from_data_object(
    object: &Map<String, Value>,
    fallback_mime: &str,
    path: Option<String>,
) -> Option<PartViewDto> {
    let explicit_mime = ["media_type", "mime_type", "mimeType"]
        .iter()
        .find_map(|key| non_empty_string_field(object, key))
        .or_else(|| {
            non_empty_string_field(object, "format").map(|fmt| {
                if fmt.contains('/') {
                    fmt
                } else {
                    format!("audio/{fmt}")
                }
            })
        });

    if let Some(data) = object.get("data").and_then(Value::as_str)
        && !data.is_empty()
    {
        if let Some((marker_mime, encoded_len)) = parse_media_marker(data) {
            return Some(PartViewDto::Media {
                mime: marker_mime.to_string(),
                encoded_length: Some(encoded_len),
                url: None,
                path,
            });
        }
        return Some(PartViewDto::Media {
            mime: explicit_mime.unwrap_or_else(|| fallback_mime.to_string()),
            encoded_length: Some(data.len()),
            url: None,
            path,
        });
    }

    for url_key in ["url", "uri", "file_uri", "fileUri"] {
        if let Some(url) = non_empty_string_field(object, url_key) {
            let mut media = media_from_url(&url, fallback_mime, path);
            if let (Some(mime), PartViewDto::Media { mime: current, .. }) =
                (explicit_mime, &mut media)
                && current == fallback_mime
            {
                *current = mime;
            }
            return Some(media);
        }
    }

    None
}

fn media_from_url(url: &str, fallback_mime: &str, path: Option<String>) -> PartViewDto {
    if let Some((mime, len)) = parse_media_marker(url) {
        return PartViewDto::Media {
            mime: mime.to_string(),
            encoded_length: Some(len),
            url: None,
            path,
        };
    }
    if let Some((mime, len)) = data_url_media(url) {
        return PartViewDto::Media {
            mime: mime.to_string(),
            encoded_length: Some(len),
            url: None,
            path,
        };
    }
    PartViewDto::Media {
        mime: fallback_mime.to_string(),
        encoded_length: None,
        url: Some(url.to_string()),
        path,
    }
}

fn fallback_mime_for_type(item_type: &str) -> &'static str {
    match item_type {
        "image" | "image_url" | "input_image" => "image/*",
        "audio" | "audio_url" | "input_audio" => "audio/*",
        "video" | "video_url" => "video/*",
        "document" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn has_cache_control(object: &Map<String, Value>) -> bool {
    object
        .get("cache_control")
        .is_some_and(|value| !value.is_null())
}

fn non_empty_string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn extract_response_error(response: &Value) -> Option<String> {
    let error = response.get("error")?;
    if error.is_null() {
        return None;
    }
    if let Some(text) = error.as_str() {
        return (!text.is_empty()).then(|| text.to_string());
    }
    if let Some(message) = error.get("message").and_then(Value::as_str)
        && !message.is_empty()
    {
        return Some(message.to_string());
    }
    Some(error.to_string())
}
