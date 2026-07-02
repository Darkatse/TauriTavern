use serde_json::Value;

use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

use super::reasoning::{
    collect_visible_reasoning_texts, collect_visible_reasoning_value, has_reasoning_native_state,
};

pub(in crate::infrastructure::logging::llm_api_logs) fn format_request_readable(
    source: ChatCompletionSource,
    payload: &Value,
) -> String {
    let mut out = String::new();
    append_tools_summary(&mut out, payload);

    let body = match source {
        ChatCompletionSource::Makersuite | ChatCompletionSource::VertexAi
            if payload.get("contents").is_some() =>
        {
            format_gemini_contents(payload)
        }
        _ if payload.get("input").is_some() => format_input_items(payload),
        _ if payload.get("messages").is_some() => format_messages_payload(payload),
        _ if payload.get("contents").is_some() => format_gemini_contents(payload),
        ChatCompletionSource::Claude => format_messages_payload(payload),
        _ => format_messages_payload(payload),
    };

    append_section_text(&mut out, &body);
    trim_readable(out)
}

fn append_tools_summary(out: &mut String, payload: &Value) {
    let Some(tools) = payload.get("tools").and_then(Value::as_array) else {
        return;
    };
    if tools.is_empty() {
        return;
    }

    out.push_str("[tools count=");
    out.push_str(&tools.len().to_string());
    if let Some(tool_choice) = payload
        .get("tool_choice")
        .or_else(|| payload.get("toolChoice"))
    {
        out.push_str(" tool_choice=");
        append_inline_json(out, tool_choice);
    }
    out.push_str("]\n");

    for tool in tools {
        let names = tool_names(tool);
        if names.is_empty() {
            out.push_str("- <invalid tool: missing name>\n");
            continue;
        }
        for name in names {
            out.push_str("- ");
            out.push_str(name);
            out.push('\n');
        }
    }
}

fn tool_names(tool: &Value) -> Vec<&str> {
    if let Some(name) = tool
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
    {
        return vec![name];
    }

    if let Some(name) = tool.get("name").and_then(Value::as_str) {
        return vec![name];
    }

    if let Some(declarations) = tool.get("functionDeclarations").and_then(Value::as_array) {
        return declarations
            .iter()
            .filter_map(|declaration| declaration.get("name").and_then(Value::as_str))
            .collect();
    }

    Vec::new()
}

fn trim_readable(mut out: String) -> String {
    out.truncate(out.trim_end().len());
    out
}

fn append_section_text(out: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !out.trim().is_empty() {
        out.push('\n');
    }
    out.push_str(text);
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn append_inline_json(out: &mut String, value: &Value) {
    if let Some(text) = value.as_str() {
        out.push_str(text);
        return;
    }
    out.push_str(&compact_json(value));
}

fn append_block_json(out: &mut String, value: &Value) {
    if let Some(text) = value.as_str() {
        out.push_str(text);
        return;
    }
    out.push_str(&compact_json(value));
}

fn append_tool_arguments(out: &mut String, tool_name: Option<&str>, arguments: &Value) {
    if !matches!(
        tool_name,
        Some("workspace_write_file" | "workspace.write_file")
    ) {
        append_block_json(out, arguments);
        return;
    }

    if let Some(text) = arguments.as_str() {
        match serde_json::from_str::<Value>(text) {
            Ok(parsed) => append_workspace_write_file_arguments(out, &parsed),
            Err(error) => {
                append_json_error_marker(out, "invalid_workspace_write_file_arguments_json", error);
                begin_block(out);
                out.push_str(text);
            }
        }
        return;
    }

    append_workspace_write_file_arguments(out, arguments);
}

fn append_workspace_write_file_arguments(out: &mut String, arguments: &Value) {
    let Some(object) = arguments.as_object() else {
        append_marker(
            out,
            "invalid_workspace_write_file_arguments: expected object",
        );
        begin_block(out);
        append_block_json(out, arguments);
        return;
    };

    let Some(content) = object.get("content").and_then(Value::as_str) else {
        append_marker(
            out,
            "invalid_workspace_write_file_arguments: missing content",
        );
        begin_block(out);
        append_block_json(out, arguments);
        return;
    };

    let mut metadata = object.clone();
    metadata.remove("content");
    if !metadata.is_empty() {
        append_block_json(out, &Value::Object(metadata));
    }

    begin_block(out);
    out.push_str("[content]\n");
    out.push_str(content);
}

fn append_json_error_marker(out: &mut String, marker: &str, error: serde_json::Error) {
    begin_block(out);
    out.push('[');
    out.push_str(marker);
    out.push_str(" error=");
    out.push_str(&compact_json(&Value::String(error.to_string())));
    out.push(']');
}

fn begin_block(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn format_messages_payload(payload: &Value) -> String {
    let mut out = String::new();

    if let Some(system) = payload.get("system") {
        out.push_str("[system]\n");
        append_message_content(&mut out, Some(system));
        out.push_str("\n\n");
    }

    let Some(messages) = payload.get("messages") else {
        return "<messages unavailable>".to_string();
    };
    let Some(messages) = messages.as_array() else {
        return "<messages invalid: expected array>".to_string();
    };

    for message in messages {
        let Some(message) = message.as_object() else {
            out.push_str("[invalid_message: expected object]\n\n");
            continue;
        };
        append_message(&mut out, message);
        out.push_str("\n\n");
    }

    trim_readable(out)
}

fn message_header(message: &serde_json::Map<String, Value>) -> String {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut header = String::new();
    header.push('[');
    header.push_str(role);

    if role == "tool" {
        append_header_attr(&mut header, "id", message.get("tool_call_id"));
        append_header_attr(&mut header, "name", message.get("name"));
    }

    header.push(']');
    header
}

fn append_message(out: &mut String, message: &serde_json::Map<String, Value>) {
    out.push_str(&message_header(message));
    out.push('\n');
    let has_reasoning =
        append_reasoning_value(out, "reasoning", message.get("reasoning_content"), false);
    if has_reasoning && content_has_displayable_value(message.get("content")) {
        begin_block(out);
        out.push_str("[content]\n");
    }
    append_message_content(out, message.get("content"));
    append_openai_tool_calls(out, message.get("tool_calls"));
}

fn content_has_displayable_value(content: Option<&Value>) -> bool {
    let Some(content) = content else {
        return false;
    };
    match content {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(object) => !object.is_empty(),
        _ => true,
    }
}

fn append_message_content(out: &mut String, content: Option<&Value>) {
    let Some(content) = content else {
        return;
    };
    if content.is_null() {
        return;
    }

    if let Some(text) = content.as_str() {
        out.push_str(text);
        return;
    }

    if let Some(items) = content.as_array() {
        for item in items {
            append_content_item(out, item);
        }
        return;
    }

    append_block_json(out, content);
}

fn append_content_item(out: &mut String, item: &Value) {
    if let Some(text) = item.as_str() {
        out.push_str(text);
        return;
    }

    let Some(object) = item.as_object() else {
        append_block_json(out, item);
        return;
    };

    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("content");

    match item_type {
        "text" | "input_text" | "output_text" => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                out.push_str(text);
            } else {
                append_marker(out, "invalid_text_part: missing text");
            }
        }
        "reasoning" | "thinking" => append_reasoning_part(out, item_type, object),
        "redacted_thinking" => append_marker(out, "redacted_thinking"),
        "tool_use" => append_claude_tool_use(out, object),
        "tool_result" => append_claude_tool_result(out, object),
        "function_call" => append_function_call_item(out, object),
        "function_result" | "function_call_output" => append_function_result_item(out, object),
        _ => {
            begin_block(out);
            out.push('[');
            out.push_str(item_type);
            out.push_str("]\n");
            append_block_json(out, item);
        }
    }
}

fn append_reasoning_part(
    out: &mut String,
    item_type: &str,
    object: &serde_json::Map<String, Value>,
) {
    append_reasoning_texts(
        out,
        item_type,
        collect_visible_reasoning_texts(object),
        has_reasoning_native_state(object),
    );
}

fn append_reasoning_value(
    out: &mut String,
    item_type: &str,
    value: Option<&Value>,
    native_state: bool,
) -> bool {
    let texts = value
        .map(collect_visible_reasoning_value)
        .unwrap_or_default();
    append_reasoning_texts(out, item_type, texts, native_state)
}

fn append_reasoning_texts(
    out: &mut String,
    item_type: &str,
    texts: Vec<String>,
    native_state: bool,
) -> bool {
    if texts.is_empty() && !native_state {
        return false;
    }

    begin_block(out);
    out.push('[');
    out.push_str(item_type);
    if native_state {
        out.push_str(" native_state=present");
    }
    out.push(']');

    if !texts.is_empty() {
        out.push('\n');
        out.push_str(&texts.join("\n\n"));
    }
    true
}

fn append_claude_tool_use(out: &mut String, object: &serde_json::Map<String, Value>) {
    begin_block(out);
    out.push_str("[tool_use");
    append_header_attr(out, "id", object.get("id"));
    append_header_attr(out, "name", object.get("name"));
    out.push(']');
    if let Some(input) = object.get("input") {
        out.push('\n');
        append_tool_arguments(out, object.get("name").and_then(Value::as_str), input);
    }
}

fn append_claude_tool_result(out: &mut String, object: &serde_json::Map<String, Value>) {
    begin_block(out);
    out.push_str("[tool_result");
    append_header_attr(out, "tool_use_id", object.get("tool_use_id"));
    append_header_attr(out, "is_error", object.get("is_error"));
    out.push(']');
    if let Some(content) = object.get("content") {
        out.push('\n');
        append_message_content(out, Some(content));
    }
}

fn append_function_call_item(out: &mut String, object: &serde_json::Map<String, Value>) {
    let name = object.get("name").and_then(Value::as_str);

    begin_block(out);
    out.push_str("[function_call");
    append_header_attr(out, "id", object.get("id"));
    append_header_attr(out, "call_id", object.get("call_id"));
    append_header_attr(out, "name", object.get("name"));
    out.push(']');

    if let Some(arguments) = object
        .get("arguments")
        .or_else(|| object.get("args"))
        .or_else(|| object.get("input"))
    {
        begin_block(out);
        append_tool_arguments(out, name, arguments);
    }
}

fn append_function_result_item(out: &mut String, object: &serde_json::Map<String, Value>) {
    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function_result");
    begin_block(out);
    out.push('[');
    out.push_str(item_type);
    append_header_attr(out, "id", object.get("id"));
    append_header_attr(out, "call_id", object.get("call_id"));
    append_header_attr(out, "name", object.get("name"));
    out.push(']');

    if let Some(result) = object
        .get("result")
        .or_else(|| object.get("output"))
        .or_else(|| object.get("response"))
    {
        begin_block(out);
        append_message_content(out, Some(result));
    }
}

fn append_openai_tool_calls(out: &mut String, tool_calls: Option<&Value>) {
    let Some(tool_calls) = tool_calls else {
        return;
    };
    let Some(calls) = tool_calls.as_array() else {
        append_marker(out, "invalid_tool_calls: expected array");
        return;
    };

    for call in calls {
        append_openai_tool_call(out, call);
    }
}

fn append_openai_tool_call(out: &mut String, call: &Value) {
    let Some(object) = call.as_object() else {
        append_marker(out, "invalid_tool_call: expected object");
        return;
    };

    let function = object.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str);

    begin_block(out);
    out.push_str("[tool_call");
    append_header_attr(out, "id", object.get("id"));
    if let Some(name) = name {
        out.push_str(" name=");
        out.push_str(name);
    } else {
        out.push_str(" invalid=missing_name");
    }
    out.push(']');

    if let Some(arguments) = function.and_then(|function| function.get("arguments")) {
        begin_block(out);
        append_tool_arguments(out, name, arguments);
    }
}

fn append_header_attr(out: &mut String, name: &str, value: Option<&Value>) {
    let Some(value) = value else {
        return;
    };
    if value.is_null() {
        return;
    }
    out.push(' ');
    out.push_str(name);
    out.push('=');
    append_inline_json(out, value);
}

fn append_marker(out: &mut String, marker: &str) {
    begin_block(out);
    out.push('[');
    out.push_str(marker);
    out.push(']');
}

fn format_input_items(payload: &Value) -> String {
    let Some(input) = payload.get("input") else {
        return "<input unavailable>".to_string();
    };

    let mut out = String::new();

    if let Some(system_instruction) = payload
        .get("system_instruction")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("[system]\n");
        out.push_str(system_instruction);
        out.push_str("\n\n");
    }

    match input {
        Value::String(text) => {
            out.push_str("[user]\n");
            out.push_str(text);
        }
        Value::Array(items) => {
            for item in items {
                let Some(object) = item.as_object() else {
                    out.push_str("[invalid_input_item: expected object]\n\n");
                    continue;
                };

                if object.get("role").and_then(Value::as_str).is_some() {
                    append_message(&mut out, object);
                    out.push_str("\n\n");
                    continue;
                }

                if let Some(ty) = object.get("type").and_then(Value::as_str) {
                    match ty {
                        "function_call" => append_function_call_item(&mut out, object),
                        "function_call_output" | "function_result" => {
                            append_function_result_item(&mut out, object)
                        }
                        "message" => append_message_content(&mut out, object.get("content")),
                        "reasoning" | "thinking" => append_reasoning_part(&mut out, ty, object),
                        _ => {
                            out.push('[');
                            out.push_str(ty);
                            out.push_str("]\n");
                            append_block_json(&mut out, item);
                        }
                    }
                    out.push_str("\n\n");
                }
            }
        }
        _ => return "<input unavailable>".to_string(),
    }

    out.truncate(out.trim_end().len());
    out
}

fn format_gemini_contents(payload: &Value) -> String {
    let Some(contents_value) = payload.get("contents") else {
        return "<contents unavailable>".to_string();
    };
    let Some(contents) = contents_value.as_array() else {
        return "<contents invalid: expected array>".to_string();
    };

    let mut out = String::new();
    for content in contents {
        let Some(content) = content.as_object() else {
            out.push_str("[invalid_content: expected object]\n\n");
            continue;
        };
        let role = content
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        out.push('[');
        out.push_str(role);
        out.push_str("]\n");
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            out.push_str("\n\n");
            continue;
        };
        for part in parts {
            append_gemini_part(&mut out, part);
        }
        out.push_str("\n\n");
    }
    out.truncate(out.trim_end().len());
    out
}

fn append_gemini_part(out: &mut String, part: &Value) {
    let Some(object) = part.as_object() else {
        append_block_json(out, part);
        return;
    };

    let is_thought = object
        .get("thought")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if is_thought {
        append_reasoning_part(out, "thought", object);
        return;
    }

    if let Some(text) = object.get("text").and_then(Value::as_str) {
        begin_block(out);
        out.push_str(text);
        return;
    }

    if let Some(function_call) = object.get("functionCall").and_then(Value::as_object) {
        let name = function_call.get("name").and_then(Value::as_str);

        begin_block(out);
        out.push_str("[functionCall");
        append_header_attr(out, "id", function_call.get("id"));
        append_header_attr(out, "name", function_call.get("name"));
        out.push(']');
        if let Some(args) = function_call.get("args") {
            begin_block(out);
            append_tool_arguments(out, name, args);
        }
        return;
    }

    if let Some(function_response) = object.get("functionResponse").and_then(Value::as_object) {
        begin_block(out);
        out.push_str("[functionResponse");
        append_header_attr(out, "id", function_response.get("id"));
        append_header_attr(out, "name", function_response.get("name"));
        out.push(']');
        if let Some(response) = function_response.get("response") {
            begin_block(out);
            append_block_json(out, response);
        }
        return;
    }

    if object.get("thoughtSignature").is_some() {
        append_reasoning_part(out, "thought", object);
        return;
    }

    begin_block(out);
    out.push_str("[part]\n");
    append_block_json(out, part);
}

pub(in crate::infrastructure::logging::llm_api_logs) fn format_response_readable(
    response: &Value,
) -> String {
    if response.get("choices").is_some() {
        return format_openai_response(response);
    }
    if response.get("output").is_some() {
        return format_responses_output(response);
    }
    if response.get("content").is_some() {
        return format_claude_response(response);
    }
    if response.get("candidates").is_some() {
        return format_gemini_response(response);
    }
    "<response unavailable>".to_string()
}

fn format_openai_response(response: &Value) -> String {
    let Some(choices) = response.get("choices") else {
        return "<choices unavailable>".to_string();
    };
    let Some(choices) = choices.as_array() else {
        return "<choices invalid: expected array>".to_string();
    };

    let mut out = String::new();
    for (index, choice) in choices.iter().enumerate() {
        let Some(message) = choice.get("message").and_then(Value::as_object) else {
            out.push_str("[invalid_choice index=");
            out.push_str(&index.to_string());
            out.push_str(": missing message]\n\n");
            continue;
        };
        append_message(&mut out, message);
        out.push_str("\n\n");
    }
    trim_readable(out)
}

fn format_responses_output(response: &Value) -> String {
    let Some(output) = response.get("output") else {
        return "<output unavailable>".to_string();
    };
    let Some(items) = output.as_array() else {
        return "<output invalid: expected array>".to_string();
    };

    let mut out = String::new();
    for item in items {
        let Some(object) = item.as_object() else {
            out.push_str("[invalid_output_item: expected object]\n\n");
            continue;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("message") => {
                out.push_str("[message");
                append_header_attr(&mut out, "id", object.get("id"));
                append_header_attr(&mut out, "status", object.get("status"));
                out.push_str("]\n");
                let has_reasoning = append_reasoning_value(
                    &mut out,
                    "reasoning",
                    object.get("reasoning_content"),
                    false,
                );
                if has_reasoning && content_has_displayable_value(object.get("content")) {
                    begin_block(&mut out);
                    out.push_str("[content]\n");
                }
                append_message_content(&mut out, object.get("content"));
            }
            Some("function_call") => append_function_call_item(&mut out, object),
            Some("function_call_output") | Some("function_result") => {
                append_function_result_item(&mut out, object)
            }
            Some(kind @ ("reasoning" | "thinking")) => {
                append_reasoning_part(&mut out, kind, object)
            }
            Some(kind) => {
                out.push('[');
                out.push_str(kind);
                out.push_str("]\n");
                append_block_json(&mut out, item);
            }
            None => {
                out.push_str("[invalid_output_item: missing type]\n");
                append_block_json(&mut out, item);
            }
        }
        out.push_str("\n\n");
    }
    trim_readable(out)
}

fn format_claude_response(response: &Value) -> String {
    let mut out = String::new();
    out.push_str("[assistant]\n");
    append_message_content(&mut out, response.get("content"));
    trim_readable(out)
}

fn format_gemini_response(response: &Value) -> String {
    let Some(candidates) = response.get("candidates") else {
        return "<candidates unavailable>".to_string();
    };
    let Some(candidates) = candidates.as_array() else {
        return "<candidates invalid: expected array>".to_string();
    };

    let mut out = String::new();
    for (index, candidate) in candidates.iter().enumerate() {
        out.push_str("[candidate ");
        out.push_str(&index.to_string());
        out.push_str("]\n");
        if let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                append_gemini_part(&mut out, part);
            }
        } else {
            out.push_str("[invalid_candidate: missing content.parts]");
        }
        out.push_str("\n\n");
    }
    trim_readable(out)
}
