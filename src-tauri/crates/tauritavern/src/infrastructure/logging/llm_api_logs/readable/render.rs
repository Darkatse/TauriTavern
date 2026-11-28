use serde_json::{Map, Value};
use tt_contracts::observability::{
    MessageViewDto, PartViewDto, RequestKindDto, RequestViewDto, ResponseViewDto,
};

pub fn render_request_readable(request: &RequestViewDto) -> String {
    if request.kind == RequestKindDto::Text {
        return request
            .prompt
            .clone()
            .unwrap_or_else(|| "<prompt unavailable>".to_string());
    }

    let mut out = String::new();
    append_tools_summary(&mut out, request);

    let mut body = String::new();
    if !request.system.is_empty() {
        body.push_str("[system]\n");
        render_parts(&mut body, &request.system);
        body.push_str("\n\n");
    }

    for message in &request.messages {
        render_chat_message(&mut body, message);
        body.push_str("\n\n");
    }

    let body = body.trim();
    if !body.is_empty() {
        if !out.trim().is_empty() {
            out.push('\n');
        }
        out.push_str(body);
    }

    trim_readable(out)
}

pub fn render_response_readable(kind: RequestKindDto, response: &ResponseViewDto) -> String {
    if response.candidates.is_empty() {
        return response.error.clone().unwrap_or_default();
    }

    if kind == RequestKindDto::Text {
        let rendered = response
            .candidates
            .iter()
            .map(render_text_candidate)
            .collect::<Vec<_>>()
            .join("\n\n");
        return append_trailing_error(rendered, response.error.as_deref());
    }

    let mut out = String::new();
    for candidate in &response.candidates {
        render_chat_message(&mut out, candidate);
        out.push_str("\n\n");
    }
    let rendered = trim_readable(out);
    append_trailing_error(rendered, response.error.as_deref())
}

fn append_trailing_error(mut rendered: String, error: Option<&str>) -> String {
    let Some(error) = error.filter(|err| !err.is_empty()) else {
        return rendered;
    };
    if !rendered.is_empty() {
        rendered.push_str("\n\n[error]\n");
    }
    rendered.push_str(error);
    rendered
}

fn render_text_candidate(candidate: &MessageViewDto) -> String {
    let mut reasoning = String::new();
    let mut text = String::new();

    for part in &candidate.parts {
        match part {
            PartViewDto::Reasoning { text: chunk, .. } => reasoning.push_str(chunk),
            PartViewDto::Text { text: chunk, .. } => text.push_str(chunk),
            _ => {}
        }
    }

    match (reasoning.trim().is_empty(), text.trim().is_empty()) {
        (true, true) => String::new(),
        (true, false) => text,
        (false, true) => format!("[reasoning]\n{reasoning}"),
        (false, false) => format!("[reasoning]\n{reasoning}\n\n[assistant]\n{text}"),
    }
}

fn append_tools_summary(out: &mut String, request: &RequestViewDto) {
    if request.tools.is_empty() {
        return;
    }

    out.push_str("[tools count=");
    out.push_str(&request.tools.len().to_string());
    if let Some(tool_choice) = &request.tool_choice {
        out.push_str(" tool_choice=");
        append_value(out, tool_choice);
    }
    out.push_str("]\n");

    for tool in &request.tools {
        out.push_str("- ");
        out.push_str(&tool.name);
        out.push('\n');
    }
}

fn render_chat_message(out: &mut String, message: &MessageViewDto) {
    if message.role == "tool"
        && let [
            PartViewDto::ToolResult {
                call_id,
                name,
                is_error,
                parts,
                ..
            },
        ] = message.parts.as_slice()
    {
        push_tool_result_marker(out, "tool", call_id, name.as_deref(), *is_error);
        render_parts(out, parts);
        return;
    }

    out.push('[');
    out.push_str(&message.role);
    out.push_str("]\n");
    render_parts(out, &message.parts);
}

fn render_parts(out: &mut String, parts: &[PartViewDto]) {
    let mut saw_reasoning = false;
    let mut after_text = false;

    for part in parts {
        // A part starts on its own line; text only runs on from the text before it.
        let is_text = matches!(part, PartViewDto::Text { .. });
        if !(is_text && after_text) {
            begin_block(out);
        }
        after_text = is_text;

        match part {
            PartViewDto::Reasoning { text, .. } => {
                out.push_str("[reasoning]");
                if !text.is_empty() {
                    out.push('\n');
                    out.push_str(text);
                }
                saw_reasoning = true;
            }
            PartViewDto::NativeState { kind, .. } => {
                out.push_str("[native_state kind=");
                out.push_str(kind);
                out.push(']');
            }
            PartViewDto::Text { text, .. } => {
                if saw_reasoning && !text.trim().is_empty() {
                    begin_block(out);
                    out.push_str("[content]\n");
                    saw_reasoning = false;
                }
                out.push_str(text);
            }
            PartViewDto::Media {
                mime,
                encoded_length,
                url,
                ..
            } => {
                out.push_str("[media mime=");
                out.push_str(mime);
                if let Some(len) = encoded_length {
                    out.push_str(" base64_chars=");
                    out.push_str(&len.to_string());
                }
                if let Some(url) = url {
                    out.push_str(" url=");
                    out.push_str(url);
                }
                out.push(']');
            }
            PartViewDto::ToolCall {
                id,
                name,
                arguments,
                arguments_text,
                ..
            } => {
                out.push_str("[tool_call");
                if !id.is_empty() {
                    out.push_str(" id=");
                    out.push_str(id);
                }
                out.push_str(" name=");
                out.push_str(name);
                out.push(']');
                render_tool_arguments(out, arguments, arguments_text.as_deref());
            }
            PartViewDto::ToolResult {
                call_id,
                name,
                is_error,
                parts: inner,
                ..
            } => {
                push_tool_result_marker(out, "tool_result", call_id, name.as_deref(), *is_error);
                render_parts(out, inner);
            }
            PartViewDto::Other { label, value, .. } => {
                out.push('[');
                out.push_str(label);
                out.push_str("]\n");
                append_value(out, value);
            }
        }
    }
}

fn push_tool_result_marker(
    out: &mut String,
    marker: &str,
    call_id: &str,
    name: Option<&str>,
    is_error: Option<bool>,
) {
    out.push('[');
    out.push_str(marker);
    if !call_id.is_empty() {
        out.push_str(" id=");
        out.push_str(call_id);
    }
    if let Some(name) = name {
        out.push_str(" name=");
        out.push_str(name);
    }
    if let Some(is_error) = is_error {
        out.push_str(" is_error=");
        out.push_str(if is_error { "true" } else { "false" });
    }
    out.push(']');
}

/// Expands any multiline string field in an object argument into `[key]\nvalue` blocks.
fn render_tool_arguments(out: &mut String, arguments: &Value, arguments_text: Option<&str>) {
    if let Some(raw_text) = arguments_text {
        begin_block(out);
        out.push_str("[invalid_arguments_json]\n");
        out.push_str(raw_text);
        return;
    }

    let Some(object) = arguments.as_object() else {
        begin_block(out);
        append_value(out, arguments);
        return;
    };

    let mut inline = Map::new();
    let mut multiline = Vec::<(&str, &str)>::new();
    for (key, value) in object {
        if let Some(text) = value.as_str()
            && text.contains('\n')
        {
            multiline.push((key.as_str(), text));
        } else {
            inline.insert(key.clone(), value.clone());
        }
    }

    if multiline.is_empty() {
        begin_block(out);
        out.push_str(&arguments.to_string());
        return;
    }

    if !inline.is_empty() {
        begin_block(out);
        out.push_str(&Value::Object(inline).to_string());
    }

    for (key, text) in multiline {
        begin_block(out);
        out.push('[');
        out.push_str(key);
        out.push_str("]\n");
        out.push_str(text);
    }
}

fn append_value(out: &mut String, value: &Value) {
    if let Some(text) = value.as_str() {
        out.push_str(text);
    } else {
        out.push_str(&value.to_string());
    }
}

fn begin_block(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn trim_readable(mut out: String) -> String {
    out.truncate(out.trim_end().len());
    out
}
