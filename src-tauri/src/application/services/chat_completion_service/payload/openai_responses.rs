use serde_json::{Map, Number, Value, json};

use crate::application::errors::ApplicationError;

use super::shared::{apply_custom_body_overrides, message_content_to_text};
use super::tool_calls::message_tool_call_id;

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

    let request = build_openai_responses_payload(&payload)?;

    let mut upstream_payload = Value::Object(request);
    apply_custom_body_overrides(&mut upstream_payload, &include_raw, &exclude_raw)?;

    Ok(("/responses".to_string(), upstream_payload))
}

fn build_openai_responses_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "OpenAI Responses request is missing model".to_string(),
            )
        })?;

    let input = build_input_items(payload.get("messages"))?;

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));
    request.insert("input".to_string(), Value::Array(input));

    request.insert("store".to_string(), Value::Bool(false));

    for key in ["stream", "temperature", "top_p", "seed"] {
        if let Some(value) = payload.get(key).filter(|value| !value.is_null()) {
            request.insert(key.to_string(), value.clone());
        }
    }

    if let Some(max_tokens) = payload
        .get("max_tokens")
        .or_else(|| payload.get("max_completion_tokens"))
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
    {
        request.insert(
            "max_output_tokens".to_string(),
            Value::Number(Number::from(max_tokens)),
        );
    }

    if let Some(reasoning_effort) = payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"))
    {
        let effort = if reasoning_effort.eq_ignore_ascii_case("min") {
            "minimal"
        } else {
            reasoning_effort
        };
        request.insert("reasoning".to_string(), json!({ "effort": effort }));
    }

    if let Some(verbosity) = payload
        .get("verbosity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"))
    {
        request.insert(
            "verbosity".to_string(),
            Value::String(verbosity.to_string()),
        );
    }

    if let Some(tools) = payload.get("tools").and_then(Value::as_array) {
        if !tools.is_empty() {
            request.insert(
                "tools".to_string(),
                Value::Array(map_openai_tools_to_responses(tools)),
            );

            if let Some(tool_choice) = payload.get("tool_choice") {
                request.insert(
                    "tool_choice".to_string(),
                    map_openai_tool_choice_to_responses(tool_choice.clone()),
                );
            }
        }
    }

    request.remove(CUSTOM_API_FORMAT);

    Ok(request)
}

fn build_input_items(messages: Option<&Value>) -> Result<Vec<Value>, ApplicationError> {
    let Some(messages) = messages else {
        return Ok(Vec::new());
    };

    if let Some(prompt) = messages.as_str() {
        return Ok(vec![json!({
            "role": "user",
            "content": prompt,
        })]);
    }

    let Some(entries) = messages.as_array() else {
        return Ok(Vec::new());
    };

    let trailing_tool_messages = entries
        .iter()
        .rev()
        .take_while(|entry| {
            entry
                .as_object()
                .and_then(|object| object.get("role"))
                .and_then(Value::as_str)
                .map(str::trim)
                .map(|role| {
                    role.eq_ignore_ascii_case("tool") || role.eq_ignore_ascii_case("function")
                })
                .unwrap_or(false)
        })
        .count();
    let trailing_tool_start = entries.len().saturating_sub(trailing_tool_messages);

    let mut input = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let raw_role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim();

        if raw_role.eq_ignore_ascii_case("assistant") {
            if let Some(native_output) = message_native_openai_responses_output(message) {
                input.extend(native_output);
                continue;
            }
        }

        if raw_role.eq_ignore_ascii_case("tool") || raw_role.eq_ignore_ascii_case("function") {
            if index >= trailing_tool_start {
                let call_id = message_tool_call_id(message).ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "Tool message is missing tool_call_id required for Responses function_call_output".to_string(),
                    )
                })?;

                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": message_content_to_text(message.get("content")),
                }));
            } else {
                input.push(json!({
                    "role": "user",
                    "content": message_content_to_text(message.get("content")),
                }));
            }

            continue;
        }

        let mapped_role = if raw_role.eq_ignore_ascii_case("system") {
            "developer"
        } else {
            raw_role
        };

        input.push(json!({
            "role": mapped_role,
            "content": message_content_to_text(message.get("content")),
        }));
    }

    Ok(input)
}

fn message_native_openai_responses_output(message: &Map<String, Value>) -> Option<Vec<Value>> {
    message
        .get("native")?
        .get("openai_responses")?
        .get("output")?
        .as_array()
        .cloned()
}

fn map_openai_tools_to_responses(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| tool.as_object())
        .map(|tool| {
            let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or_default();

            if tool_type != "function" {
                return Value::Object(tool.clone());
            }

            let strict = tool
                .get("strict")
                .and_then(Value::as_bool)
                .or_else(|| {
                    tool.get("function")
                        .and_then(Value::as_object)
                        .and_then(|f| f.get("strict"))
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false);

            if let Some(function) = tool.get("function").and_then(Value::as_object) {
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
                mapped.insert("strict".to_string(), Value::Bool(strict));
                return Value::Object(mapped);
            }

            let mut mapped = Map::new();
            mapped.insert("type".to_string(), Value::String("function".to_string()));
            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                mapped.insert("name".to_string(), Value::String(name.to_string()));
            }
            if let Some(description) = tool.get("description").and_then(Value::as_str) {
                mapped.insert(
                    "description".to_string(),
                    Value::String(description.to_string()),
                );
            }
            if let Some(parameters) = tool.get("parameters") {
                mapped.insert("parameters".to_string(), parameters.clone());
            }
            mapped.insert("strict".to_string(), Value::Bool(strict));
            Value::Object(mapped)
        })
        .collect()
}

fn map_openai_tool_choice_to_responses(tool_choice: Value) -> Value {
    let Value::Object(object) = tool_choice else {
        return tool_choice;
    };

    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if tool_type == "function" {
        if let Some(function) = object.get("function").and_then(Value::as_object) {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                return json!({
                    "type": "function",
                    "name": name,
                });
            }
        }

        if object.get("name").and_then(Value::as_str).is_some() {
            return Value::Object(object);
        }
    }

    if tool_type == "allowed_tools" {
        let mut mapped = object.clone();
        if let Some(tools) = mapped.get_mut("tools").and_then(Value::as_array_mut) {
            for tool in tools.iter_mut() {
                let Value::Object(tool_object) = tool else {
                    continue;
                };
                if tool_object.get("type").and_then(Value::as_str) != Some("function") {
                    continue;
                }
                if tool_object.get("name").and_then(Value::as_str).is_some() {
                    continue;
                }
                if let Some(function) = tool_object.get("function").and_then(Value::as_object) {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        tool_object.insert("name".to_string(), Value::String(name.to_string()));
                    }
                }
                tool_object.remove("function");
            }
        }
        return Value::Object(mapped);
    }

    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn openai_responses_payload_maps_system_to_developer_and_trailing_tool_output() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": "" },
                { "role": "tool", "tool_call_id": "call_123", "content": "ok" }
            ],
            "stream": true,
            "custom_url": "https://example.com/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/responses");

        let request = upstream.as_object().expect("request must be object");
        assert_eq!(request.get("model").and_then(|v| v.as_str()), Some("gpt-5"));

        let input = request
            .get("input")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        assert_eq!(input[0]["role"], "developer");
        assert_eq!(input[0]["content"], "sys");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_123");
        assert_eq!(input[3]["output"], "ok");
    }

    #[test]
    fn openai_responses_payload_lifts_function_tools() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [{ "role": "user", "content": "hi" }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "desc",
                    "parameters": { "type": "object", "properties": {} }
                }
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let request = upstream.as_object().expect("request must be object");
        let tools = request.get("tools").and_then(|v| v.as_array()).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["strict"], false);
    }

    #[test]
    fn openai_responses_payload_replays_native_output_items() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "user", "content": "hi" },
                {
                    "role": "assistant",
                    "content": "",
                    "native": {
                        "openai_responses": {
                            "responseId": "resp_1",
                            "output": [{
                                "id": "fc_1",
                                "type": "function_call",
                                "call_id": "call_1",
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"hi\"}"
                            }]
                        }
                    }
                },
                { "role": "tool", "tool_call_id": "call_1", "content": "ok" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let input = upstream
            .get("input")
            .and_then(Value::as_array)
            .expect("input should exist");

        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_1");
    }
}
