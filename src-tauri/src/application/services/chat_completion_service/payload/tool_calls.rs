use serde_json::{json, Map, Value};

use super::shared::message_content_to_text;

const DEFAULT_TOOL_NAME: &str = "tool";

#[derive(Debug, Clone)]
pub(super) struct OpenAiToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub signature: Option<String>,
}

pub(super) fn extract_openai_tool_calls(value: Option<&Value>) -> Vec<OpenAiToolCall> {
    let Some(value) = value else {
        return Vec::new();
    };

    let entries: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    };

    entries
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let object = entry.as_object()?;
            let function = object.get("function").and_then(Value::as_object)?;
            let name = non_empty_string(function.get("name"))?;

            let id =
                non_empty_string(object.get("id")).unwrap_or_else(|| format!("tool_call_{index}"));
            let arguments = parse_tool_call_arguments(
                function.get("arguments").or_else(|| function.get("args")),
            );
            let signature = non_empty_string(object.get("signature"));

            Some(OpenAiToolCall {
                id,
                name,
                arguments,
                signature,
            })
        })
        .collect()
}

pub(super) fn message_tool_call_id(message: &Map<String, Value>) -> Option<String> {
    non_empty_string(message.get("tool_call_id"))
}

pub(super) fn message_tool_name(message: &Map<String, Value>) -> Option<String> {
    non_empty_string(message.get("name"))
}

pub(super) fn message_tool_result_text(message: &Map<String, Value>) -> String {
    message_content_to_text(message.get("content"))
}

pub(super) fn normalize_tool_result_payload(content: &str) -> Value {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return json!({ "content": "" });
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(object)) => Value::Object(object),
        Ok(other) => json!({ "content": other }),
        Err(_) => json!({ "content": content }),
    }
}

pub(super) fn fallback_tool_name() -> &'static str {
    DEFAULT_TOOL_NAME
}

fn parse_tool_call_arguments(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return Value::Object(Map::new());
    };

    match value {
        Value::String(raw) => {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
        }
        Value::Null => Value::Object(Map::new()),
        other => other.clone(),
    }
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{extract_openai_tool_calls, normalize_tool_result_payload};

    #[test]
    fn extract_openai_tool_calls_parses_arguments_and_signature() {
        let value = json!([{
            "id": "call_1",
            "type": "function",
            "function": {
                "name": "weather",
                "arguments": "{\"city\":\"Paris\"}"
            },
            "signature": "sig_1"
        }]);

        let calls = extract_openai_tool_calls(Some(&value));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "weather");
        assert_eq!(calls[0].signature.as_deref(), Some("sig_1"));
        assert_eq!(calls[0].arguments["city"], "Paris");
    }

    #[test]
    fn normalize_tool_result_payload_wraps_plain_text() {
        let payload = normalize_tool_result_payload("done");
        assert_eq!(payload["content"], "done");
    }
}
