use std::collections::HashMap;

use serde_json::{Map, Number, Value, json};

use crate::application::errors::ApplicationError;

use super::shared::{apply_custom_body_overrides, message_content_to_text, parse_data_url};
use super::tool_calls::{
    OpenAiToolCall, extract_openai_tool_calls, fallback_tool_name, message_tool_call_id,
    message_tool_name, message_tool_result_text, normalize_tool_result_payload,
};

const CUSTOM_API_FORMAT: &str = "custom_api_format";

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_raw = payload
        .get("custom_include_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let exclude_raw = payload
        .get("custom_exclude_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let request = build_gemini_interactions_payload(&payload)?;

    let mut upstream_payload = Value::Object(request);
    apply_custom_body_overrides(&mut upstream_payload, &include_raw, &exclude_raw)?;

    Ok(("/interactions".to_string(), upstream_payload))
}

fn build_gemini_interactions_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Gemini Interactions request is missing model".to_string())
        })?;

    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let (input, system_instruction) = build_input_and_system_instruction(payload.get("messages"))?;

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));
    request.insert("input".to_string(), input);
    request.insert("stream".to_string(), Value::Bool(stream));
    request.insert("store".to_string(), Value::Bool(false));

    if let Some(system_instruction) = system_instruction {
        request.insert(
            "system_instruction".to_string(),
            Value::String(system_instruction),
        );
    }

    if let Some(generation_config) = build_generation_config(payload) {
        request.insert(
            "generation_config".to_string(),
            Value::Object(generation_config),
        );
    }

    if let Some(tools) = payload.get("tools").and_then(Value::as_array) {
        if !tools.is_empty() {
            request.insert(
                "tools".to_string(),
                Value::Array(map_openai_tools_to_interactions(tools)),
            );
        }
    }

    if let Some(schema_value) = payload
        .get("json_schema")
        .and_then(Value::as_object)
        .and_then(|schema| schema.get("value"))
        .filter(|value| !value.is_null())
    {
        request.insert("response_format".to_string(), schema_value.clone());
    }

    request.remove(CUSTOM_API_FORMAT);

    Ok(request)
}

fn build_generation_config(payload: &Map<String, Value>) -> Option<Map<String, Value>> {
    let mut config = Map::new();

    if let Some(temperature) = payload.get("temperature").filter(|value| !value.is_null()) {
        config.insert("temperature".to_string(), temperature.clone());
    }

    if let Some(top_p) = payload.get("top_p").filter(|value| !value.is_null()) {
        config.insert("top_p".to_string(), top_p.clone());
    }

    if let Some(top_k) = payload
        .get("top_k")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
    {
        config.insert("top_k".to_string(), Value::Number(Number::from(top_k)));
    }

    if let Some(max_tokens) = payload
        .get("max_output_tokens")
        .or_else(|| payload.get("max_tokens"))
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
    {
        config.insert(
            "max_output_tokens".to_string(),
            Value::Number(Number::from(max_tokens)),
        );
    }

    if config.is_empty() {
        None
    } else {
        Some(config)
    }
}

fn map_openai_tools_to_interactions(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| tool.as_object())
        .map(|tool| {
            let tool_type = tool
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if tool_type != "function" {
                return Value::Object(tool.clone());
            }

            let Some(function) = tool.get("function").and_then(Value::as_object) else {
                return Value::Object(tool.clone());
            };

            let mut mapped = Map::new();
            mapped.insert("type".to_string(), Value::String("function".to_string()));
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                mapped.insert("name".to_string(), Value::String(name.to_string()));
            }
            if let Some(description) = function.get("description").and_then(Value::as_str) {
                mapped.insert(
                    "description".to_string(),
                    Value::String(description.to_string()),
                );
            }
            if let Some(parameters) = function.get("parameters") {
                mapped.insert("parameters".to_string(), parameters.clone());
            }

            Value::Object(mapped)
        })
        .collect()
}

fn build_input_and_system_instruction(
    messages: Option<&Value>,
) -> Result<(Value, Option<String>), ApplicationError> {
    let Some(messages) = messages else {
        return Ok((Value::Array(Vec::new()), None));
    };

    if let Some(prompt) = messages.as_str() {
        return Ok((Value::String(prompt.to_string()), None));
    }

    let Some(entries) = messages.as_array() else {
        return Ok((Value::Array(Vec::new()), None));
    };

    let mut turns = Vec::new();
    let mut system_parts = Vec::new();
    let mut tool_name_by_id = HashMap::<String, String>::new();

    let mut index = 0_usize;
    while index < entries.len() {
        let Some(message) = entries[index].as_object() else {
            index += 1;
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_ascii_lowercase();

        if role == "system" {
            let text = message_content_to_text(message.get("content"));
            if !text.trim().is_empty() {
                system_parts.push(text);
            }
            index += 1;
            continue;
        }

        if role == "tool" || role == "function" {
            let mut results = Vec::new();
            while index < entries.len() {
                let Some(tool_message) = entries[index].as_object() else {
                    index += 1;
                    continue;
                };

                let tool_role = tool_message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .trim()
                    .to_ascii_lowercase();

                if tool_role != "tool" && tool_role != "function" {
                    break;
                }

                let call_id = message_tool_call_id(tool_message).ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "Tool message is missing tool_call_id required for Gemini Interactions function_result".to_string(),
                    )
                })?;

                let name = message_tool_name(tool_message)
                    .or_else(|| tool_name_by_id.get(&call_id).cloned())
                    .unwrap_or_else(|| fallback_tool_name().to_string());

                let result_text = message_tool_result_text(tool_message);
                let result_payload = normalize_tool_result_payload(&result_text);

                results.push(json!({
                    "type": "function_result",
                    "name": name,
                    "call_id": call_id,
                    "result": result_payload,
                }));

                index += 1;
            }

            turns.push(json!({
                "role": "user",
                "content": results,
            }));

            continue;
        }

        if role == "assistant" {
            if let Some(tool_calls) = message.get("tool_calls") {
                for tool_call in extract_openai_tool_calls(Some(tool_calls)) {
                    tool_name_by_id.insert(tool_call.id.clone(), tool_call.name.clone());
                }
            }

            let content = message_native_outputs(message).unwrap_or_else(|| {
                build_synthetic_model_outputs(
                    message.get("content"),
                    message.get("tool_calls"),
                    message.get("signature"),
                )
            });

            turns.push(json!({
                "role": "model",
                "content": content,
            }));

            index += 1;
            continue;
        }

        let content_blocks = convert_openai_content_to_interactions_blocks(message.get("content"));
        let content = blocks_to_interactions_content_value(content_blocks);
        turns.push(json!({
            "role": "user",
            "content": content,
        }));

        index += 1;
    }

    let system_instruction = system_parts
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    let system_instruction = if system_instruction.is_empty() {
        None
    } else {
        Some(system_instruction)
    };

    Ok((Value::Array(turns), system_instruction))
}

fn message_native_outputs(message: &Map<String, Value>) -> Option<Value> {
    let native = message.get("native")?.as_object()?;
    let interactions = native.get("gemini_interactions")?.as_object()?;
    let outputs = interactions.get("outputs")?.as_array()?.clone();

    Some(Value::Array(outputs))
}

fn build_synthetic_model_outputs(
    content: Option<&Value>,
    tool_calls: Option<&Value>,
    signature: Option<&Value>,
) -> Value {
    let mut outputs = Vec::new();

    let text = message_content_to_text(content);
    if !text.trim().is_empty() {
        outputs.push(json!({
            "type": "text",
            "text": text,
        }));
    }

    let tool_calls = extract_openai_tool_calls(tool_calls);
    if !tool_calls.is_empty() {
        outputs.extend(tool_calls.iter().map(build_function_call_output));
    }

    if let Some(signature) = signature.and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) {
        outputs.push(json!({
            "type": "thought",
            "signature": signature,
        }));
    }

    Value::Array(outputs)
}

fn build_function_call_output(tool_call: &OpenAiToolCall) -> Value {
    let mut output = json!({
        "type": "function_call",
        "id": tool_call.id.clone(),
        "name": tool_call.name.clone(),
        "arguments": tool_call.arguments.clone(),
    });

    if let Some(signature) = tool_call.signature.as_deref() {
        if let Some(object) = output.as_object_mut() {
            object.insert(
                "signature".to_string(),
                Value::String(signature.to_string()),
            );
        }
    }

    output
}

fn convert_openai_content_to_interactions_blocks(content: Option<&Value>) -> Vec<Value> {
    let Some(content) = content else {
        return Vec::new();
    };

    match content {
        Value::String(text) => {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![json!({ "type": "text", "text": text })]
            }
        }
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(text) => Some(json!({ "type": "text", "text": text })),
                Value::Object(object) => {
                    if let Some(ty) = object.get("type").and_then(Value::as_str) {
                        if ty == "text" {
                            if let Some(text) = object.get("text").and_then(Value::as_str) {
                                return Some(json!({ "type": "text", "text": text }));
                            }
                        }

                        if ty == "image_url" {
                            let url = object
                                .get("image_url")
                                .and_then(Value::as_object)
                                .and_then(|entry| entry.get("url"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())?;

                            if let Some((mime_type, data)) = parse_data_url(url) {
                                return Some(json!({
                                    "type": "image",
                                    "mime_type": mime_type,
                                    "data": data,
                                }));
                            }

                            return Some(json!({ "type": "image", "uri": url }));
                        }

                        if ty == "audio_url" {
                            let url = object
                                .get("audio_url")
                                .and_then(Value::as_object)
                                .and_then(|entry| entry.get("url"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())?;

                            if let Some((mime_type, data)) = parse_data_url(url) {
                                return Some(json!({
                                    "type": "audio",
                                    "mime_type": mime_type,
                                    "data": data,
                                }));
                            }

                            return Some(json!({ "type": "audio", "uri": url }));
                        }

                        if ty == "video_url" {
                            let url = object
                                .get("video_url")
                                .and_then(Value::as_object)
                                .and_then(|entry| entry.get("url"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())?;

                            if let Some((mime_type, data)) = parse_data_url(url) {
                                return Some(json!({
                                    "type": "video",
                                    "mime_type": mime_type,
                                    "data": data,
                                }));
                            }

                            return Some(json!({ "type": "video", "uri": url }));
                        }

                        if matches!(ty, "image" | "audio" | "video" | "text") {
                            return Some(Value::Object(object.clone()));
                        }
                    }

                    if let Some(text) = object.get("text").and_then(Value::as_str) {
                        return Some(json!({ "type": "text", "text": text }));
                    }

                    Some(Value::Object(object.clone()))
                }
                _ => None,
            })
            .collect(),
        Value::Null => Vec::new(),
        other => vec![json!({ "type": "text", "text": other.to_string() })],
    }
}

fn blocks_to_interactions_content_value(blocks: Vec<Value>) -> Value {
    if blocks.len() == 1 {
        if let Some(block_object) = blocks[0].as_object() {
            if block_object.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = block_object.get("text").and_then(Value::as_str) {
                    return Value::String(text.to_string());
                }
            }
        }
    }

    Value::Array(blocks)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build;

    #[test]
    fn gemini_interactions_build_maps_tool_calls_and_results() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "gemini_interactions",
            "model": "gemini-3-flash-preview",
            "messages": [
                { "role": "user", "content": "What is the weather in Paris?" },
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"Paris\"}"
                        },
                        "signature": "sig_1"
                    }]
                },
                { "role": "tool", "tool_call_id": "call_1", "content": "Sunny" }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Gets the weather",
                    "parameters": { "type": "object" }
                }
            }],
            "custom_url": "https://generativelanguage.googleapis.com",
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/interactions");

        let upstream = upstream.as_object().expect("upstream must be object");
        assert_eq!(upstream.get("model").and_then(|v| v.as_str()), Some("gemini-3-flash-preview"));
        assert!(upstream.get("tools").is_some());

        let input = upstream
            .get("input")
            .and_then(|v| v.as_array())
            .expect("input must be array");
        assert_eq!(input.len(), 3);

        assert_eq!(input[0].get("role").and_then(|v| v.as_str()), Some("user"));
        assert_eq!(
            input[1].get("role").and_then(|v| v.as_str()),
            Some("model")
        );

        let model_outputs = input[1]
            .get("content")
            .and_then(|v| v.as_array())
            .expect("model content must be array");
        assert_eq!(model_outputs[0].get("type").and_then(|v| v.as_str()), Some("function_call"));
        assert_eq!(model_outputs[0].get("id").and_then(|v| v.as_str()), Some("call_1"));

        let tool_turn = input[2]
            .get("content")
            .and_then(|v| v.as_array())
            .expect("tool content must be array");
        assert_eq!(
            tool_turn[0].get("type").and_then(|v| v.as_str()),
            Some("function_result")
        );
        assert_eq!(
            tool_turn[0].get("call_id").and_then(|v| v.as_str()),
            Some("call_1")
        );
    }
}
