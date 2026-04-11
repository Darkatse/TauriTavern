use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

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

pub(super) fn normalize_openai_responses_response(response: Value) -> Value {
    let output_items = response
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for (index, item) in output_items.iter().enumerate() {
        let Some(item_object) = item.as_object() else {
            continue;
        };

        match item_object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "message" => {
                if item_object.get("role").and_then(Value::as_str) != Some("assistant") {
                    continue;
                }

                let content = item_object
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();

                for part in content {
                    let Some(part_object) = part.as_object() else {
                        continue;
                    };

                    if part_object.get("type").and_then(Value::as_str) != Some("output_text") {
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
            }
            "function_call" => {
                let name = as_non_empty_str(item_object.get("name")).unwrap_or("tool");
                let call_id = as_non_empty_str(item_object.get("call_id"))
                    .map(str::to_string)
                    .or_else(|| as_non_empty_str(item_object.get("id")).map(str::to_string))
                    .unwrap_or_else(|| format!("tool_call_{index}"));
                let arguments = to_openai_arguments(
                    item_object
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| Value::String("{}".to_string())),
                );
                let signature = as_non_empty_str(item_object.get("signature")).map(str::to_string);

                tool_calls.push(build_openai_tool_call(
                    &call_id,
                    name,
                    arguments,
                    signature.as_deref(),
                ));
            }
            _ => {}
        }
    }

    let mut content = text_parts.join("\n\n");
    if content.is_empty() {
        if let Some(output_text) = response
            .get("output_text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            content = output_text.to_string();
        }
    }

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(content));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let finish_reason = if message.contains_key("tool_calls") {
        "tool_calls".to_string()
    } else {
        "stop".to_string()
    };

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
        response
            .get("id")
            .cloned()
            .unwrap_or_else(|| Value::String("openai-responses-chat-completion".to_string())),
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

    if let Some(usage) = map_openai_responses_usage(response.get("usage")) {
        normalized.insert("usage".to_string(), usage);
    }

    Value::Object(normalized)
}

pub(super) fn normalize_gemini_interactions_response(response: Value) -> Value {
    let outputs = response
        .get("outputs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for (index, output) in outputs.iter().enumerate() {
        let Some(output_object) = output.as_object() else {
            continue;
        };

        match output_object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text" => {
                if let Some(text) = output_object
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    text_parts.push(text.to_string());
                }
            }
            "thought" => {
                if let Some(summary) = output_object
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    reasoning_parts.push(summary.to_string());
                }
            }
            "function_call" => {
                let name = as_non_empty_str(output_object.get("name")).unwrap_or("tool");
                let id = as_non_empty_str(output_object.get("id"))
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("tool_call_{index}"));
                let arguments = to_openai_arguments(
                    output_object
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Map::new())),
                );
                let signature =
                    as_non_empty_str(output_object.get("signature")).map(str::to_string);

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
    if !reasoning_parts.is_empty() {
        message.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning_parts.join("\n\n")),
        );
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    if !outputs.is_empty() {
        message.insert(
            "native".to_string(),
            json!({ "gemini_interactions": { "outputs": outputs } }),
        );
    }

    let finish_reason = if message.contains_key("tool_calls") {
        "tool_calls".to_string()
    } else {
        "stop".to_string()
    };

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
        response
            .get("id")
            .cloned()
            .unwrap_or_else(|| Value::String("gemini-interactions-chat-completion".to_string())),
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
            .or_else(|| response.get("modelVersion"))
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
    );
    normalized.insert(
        "choices".to_string(),
        Value::Array(vec![Value::Object(choice)]),
    );

    if let Some(usage) = map_gemini_interactions_usage(response.get("usage")) {
        normalized.insert("usage".to_string(), usage);
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

fn map_openai_responses_usage(raw_usage: Option<&Value>) -> Option<Value> {
    let usage = raw_usage?.as_object()?;

    let prompt_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let completion_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(prompt_tokens + completion_tokens);

    Some(json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": total_tokens,
    }))
}

fn map_gemini_interactions_usage(raw_usage: Option<&Value>) -> Option<Value> {
    let usage = raw_usage?.as_object()?;

    let prompt_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let completion_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let total_tokens = usage
        .get("total_tokens")
        .or_else(|| usage.get("totalTokens"))
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
    use serde_json::{Value, json};

    use super::{
        normalize_claude_response, normalize_gemini_interactions_response,
        normalize_gemini_response, normalize_openai_responses_response,
    };

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

    #[test]
    fn normalize_openai_responses_function_call_returns_openai_tool_calls() {
        let response = json!({
            "id": "resp_1",
            "model": "gpt-5",
            "output_text": "",
            "usage": {
                "input_tokens": 5,
                "output_tokens": 3,
                "total_tokens": 8
            },
            "output": [
                {
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        { "type": "output_text", "text": "hello" }
                    ]
                },
                {
                    "id": "fc_1",
                    "type": "function_call",
                    "call_id": "call_weather",
                    "name": "weather",
                    "arguments": "{\"city\":\"Paris\"}"
                }
            ]
        });

        let normalized = normalize_openai_responses_response(response);
        assert_eq!(
            normalized.get("object").and_then(Value::as_str),
            Some("chat.completion")
        );

        let message = normalized
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(Value::as_object)
            .and_then(|choice| choice.get("message"))
            .and_then(Value::as_object)
            .expect("message should exist");

        assert_eq!(
            message.get("content").and_then(Value::as_str),
            Some("hello")
        );

        let tool_call = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .and_then(|calls| calls.first())
            .and_then(Value::as_object)
            .expect("tool call should exist");

        assert_eq!(
            tool_call.get("id").and_then(Value::as_str),
            Some("call_weather")
        );
    }

    #[test]
    fn normalize_gemini_interactions_preserves_native_outputs_and_signatures() {
        let response = json!({
            "id": "interaction_1",
            "model": "gemini-3-flash-preview",
            "usage": { "input_tokens": 10, "output_tokens": 5, "total_tokens": 15 },
            "outputs": [
                { "type": "thought", "signature": "sig_thought", "summary": "Thinking..." },
                { "type": "function_call", "id": "call_1", "name": "get_weather", "arguments": { "location": "Paris" }, "signature": "sig_fc" },
                { "type": "url_context_call", "id": "browse_001", "arguments": { "urls": ["https://example.com"] }, "signature": "sig_url" },
                { "type": "text", "text": "Done." }
            ]
        });

        let normalized = normalize_gemini_interactions_response(response);

        let message = normalized
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(Value::as_object)
            .and_then(|choice| choice.get("message"))
            .and_then(Value::as_object)
            .expect("message should exist");

        assert_eq!(
            message.get("reasoning_content").and_then(Value::as_str),
            Some("Thinking...")
        );

        let tool_call = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .and_then(|calls| calls.first())
            .and_then(Value::as_object)
            .expect("tool call should exist");

        assert_eq!(tool_call.get("id").and_then(Value::as_str), Some("call_1"));
        assert_eq!(
            tool_call.get("signature").and_then(Value::as_str),
            Some("sig_fc")
        );

        let native_outputs = message
            .get("native")
            .and_then(Value::as_object)
            .and_then(|native| native.get("gemini_interactions"))
            .and_then(Value::as_object)
            .and_then(|interactions| interactions.get("outputs"))
            .and_then(Value::as_array)
            .expect("native outputs should exist");

        assert!(
            native_outputs.iter().any(
                |output| output.get("type").and_then(Value::as_str) == Some("url_context_call")
            )
        );
    }
}
