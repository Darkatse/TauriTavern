use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

pub(super) fn normalize_claude_response(response: Value) -> Value {
    let content_blocks = response
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for (index, block) in content_blocks.iter().enumerate() {
        let Some(block_object) = block.as_object() else {
            continue;
        };

        match block_object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text" => {
                if let Some(text) = block_object
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    text_parts.push(text.to_string());
                }
            }
            "tool_use" => {
                let name = as_non_empty_str(block_object.get("name")).unwrap_or("tool");
                let id = as_non_empty_str(block_object.get("id"))
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("tool_call_{index}"));
                let arguments = to_openai_arguments(
                    block_object
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Map::new())),
                );
                let signature = as_non_empty_str(block_object.get("signature")).map(str::to_string);

                tool_calls.push(build_openai_tool_call(
                    &id,
                    name,
                    arguments,
                    signature.as_deref(),
                ));
            }
            _ => {}
        }
    }

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert(
        "content".to_string(),
        Value::String(text_parts.join("\n\n")),
    );
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let finish_reason = map_claude_finish_reason(
        response.get("stop_reason").and_then(Value::as_str),
        message.contains_key("tool_calls"),
    );

    let mut choice = Map::new();
    choice.insert(
        "index".to_string(),
        Value::Number(serde_json::Number::from(0)),
    );
    choice.insert("message".to_string(), Value::Object(message));
    choice.insert(
        "finish_reason".to_string(),
        finish_reason.map(Value::String).unwrap_or(Value::Null),
    );

    let mut normalized = Map::new();
    normalized.insert(
        "id".to_string(),
        response
            .get("id")
            .cloned()
            .unwrap_or_else(|| Value::String("claude-chat-completion".to_string())),
    );
    normalized.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    normalized.insert(
        "created".to_string(),
        Value::Number(serde_json::Number::from(current_unix_timestamp())),
    );
    normalized.insert(
        "model".to_string(),
        response
            .get("model")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
    );
    normalized.insert(
        "choices".to_string(),
        Value::Array(vec![Value::Object(choice)]),
    );

    if let Some(usage) = map_claude_usage(response.get("usage")) {
        normalized.insert("usage".to_string(), usage);
    }

    if !content_blocks.is_empty() {
        normalized.insert("content".to_string(), Value::Array(content_blocks));
    }

    Value::Object(normalized)
}

pub(super) fn normalize_gemini_response(response: Value) -> Value {
    let candidates = response
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let first_candidate = candidates
        .first()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let response_content = first_candidate
        .get("content")
        .cloned()
        .or_else(|| first_candidate.get("output").cloned());

    let parts = response_content
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for (index, part) in parts.iter().enumerate() {
        let Some(part_object) = part.as_object() else {
            continue;
        };

        if let Some(function_call) = part_object.get("functionCall").and_then(Value::as_object) {
            let name = as_non_empty_str(function_call.get("name")).unwrap_or("tool");
            let args = function_call
                .get("args")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            let arguments = to_openai_arguments(args);
            let id = as_non_empty_str(function_call.get("id"))
                .map(str::to_string)
                .or_else(|| as_non_empty_str(part_object.get("id")).map(str::to_string))
                .unwrap_or_else(|| format!("tool_call_{index}"));
            let signature = as_non_empty_str(part_object.get("thoughtSignature"));

            tool_calls.push(build_openai_tool_call(&id, name, arguments, signature));
        }

        let is_thought = part_object
            .get("thought")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if is_thought {
            continue;
        }

        if let Some(text) = part_object
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            text_parts.push(text.to_string());
        }
    }

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert(
        "content".to_string(),
        Value::String(text_parts.join("\n\n")),
    );
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let finish_reason = map_gemini_finish_reason(
        first_candidate.get("finishReason").and_then(Value::as_str),
        message.contains_key("tool_calls"),
    );

    let mut choice = Map::new();
    choice.insert(
        "index".to_string(),
        Value::Number(serde_json::Number::from(0)),
    );
    choice.insert("message".to_string(), Value::Object(message));
    choice.insert("finish_reason".to_string(), Value::String(finish_reason));

    let mut normalized = Map::new();
    normalized.insert(
        "id".to_string(),
        Value::String("gemini-chat-completion".to_string()),
    );
    normalized.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    normalized.insert(
        "created".to_string(),
        Value::Number(serde_json::Number::from(current_unix_timestamp())),
    );
    normalized.insert(
        "model".to_string(),
        response
            .get("modelVersion")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
    );
    normalized.insert(
        "choices".to_string(),
        Value::Array(vec![Value::Object(choice)]),
    );

    if let Some(usage) = map_gemini_usage(&response) {
        normalized.insert("usage".to_string(), usage);
    }

    if let Some(response_content) = response_content {
        normalized.insert("responseContent".to_string(), response_content);
    }

    Value::Object(normalized)
}

fn map_claude_finish_reason(stop_reason: Option<&str>, has_tool_calls: bool) -> Option<String> {
    if has_tool_calls {
        return Some("tool_calls".to_string());
    }

    stop_reason.map(|value| match value {
        "max_tokens" => "length".to_string(),
        "tool_use" => "tool_calls".to_string(),
        "stop_sequence" | "end_turn" => "stop".to_string(),
        other => other.to_string(),
    })
}

fn map_claude_usage(raw_usage: Option<&Value>) -> Option<Value> {
    let usage = raw_usage?.as_object()?;
    let prompt_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let completion_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();

    Some(json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": prompt_tokens + completion_tokens,
    }))
}

fn map_gemini_finish_reason(finish_reason: Option<&str>, has_tool_calls: bool) -> String {
    if has_tool_calls {
        return "tool_calls".to_string();
    }

    let value = finish_reason.unwrap_or("STOP");
    if value.eq_ignore_ascii_case("MAX_TOKENS") {
        return "length".to_string();
    }

    if value.eq_ignore_ascii_case("STOP") || value.eq_ignore_ascii_case("FINISH_REASON_UNSPECIFIED")
    {
        return "stop".to_string();
    }

    "stop".to_string()
}

fn map_gemini_usage(response: &Value) -> Option<Value> {
    let usage = response.get("usageMetadata")?.as_object()?;

    let prompt_tokens = usage
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let completion_tokens = usage
        .get("candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let total_tokens = usage
        .get("totalTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(prompt_tokens + completion_tokens);

    Some(json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": total_tokens,
    }))
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn build_openai_tool_call(
    id: &str,
    name: &str,
    arguments: String,
    signature: Option<&str>,
) -> Value {
    let mut tool_call = json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        }
    });

    if let Some(signature) = signature {
        if let Some(object) = tool_call.as_object_mut() {
            object.insert(
                "signature".to_string(),
                Value::String(signature.to_string()),
            );
        }
    }

    tool_call
}

fn as_non_empty_str(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn to_openai_arguments(value: Value) -> String {
    if value.is_string() {
        return value.as_str().unwrap_or_default().to_string();
    }

    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{normalize_claude_response, normalize_gemini_response};

    #[test]
    fn normalize_claude_tool_use_preserves_signature() {
        let response = json!({
            "id": "claude-response",
            "model": "claude-3-5-sonnet-latest",
            "content": [{
                "type": "tool_use",
                "id": "call_weather",
                "name": "weather",
                "input": { "city": "Paris" },
                "signature": "sig_1"
            }],
            "stop_reason": "tool_use"
        });

        let normalized = normalize_claude_response(response);
        let tool_call = normalized
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(Value::as_object)
            .and_then(|choice| choice.get("message"))
            .and_then(Value::as_object)
            .and_then(|message| message.get("tool_calls"))
            .and_then(Value::as_array)
            .and_then(|calls| calls.first())
            .and_then(Value::as_object)
            .expect("tool call should exist");

        assert_eq!(
            tool_call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "call_weather"
        );
        assert_eq!(
            tool_call
                .get("signature")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "sig_1"
        );
    }

    #[test]
    fn normalize_gemini_function_call_maps_thought_signature() {
        let response = json!({
            "modelVersion": "gemini-2.5-flash",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "weather",
                            "args": { "city": "Paris" }
                        },
                        "thoughtSignature": "sig_2"
                    }]
                }
            }]
        });

        let normalized = normalize_gemini_response(response);
        let tool_call = normalized
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(Value::as_object)
            .and_then(|choice| choice.get("message"))
            .and_then(Value::as_object)
            .and_then(|message| message.get("tool_calls"))
            .and_then(Value::as_array)
            .and_then(|calls| calls.first())
            .and_then(Value::as_object)
            .expect("tool call should exist");

        assert_eq!(
            tool_call
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "weather"
        );
        assert_eq!(
            tool_call
                .get("signature")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "sig_2"
        );
    }
}
