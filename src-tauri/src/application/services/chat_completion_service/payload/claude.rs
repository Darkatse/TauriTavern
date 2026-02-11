use serde_json::{json, Map, Value};

use crate::application::errors::ApplicationError;

use super::shared::{insert_if_present, message_content_to_text};

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    Ok((
        "/messages".to_string(),
        Value::Object(build_claude_payload(&payload)?),
    ))
}

fn build_claude_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Claude request is missing model".to_string())
        })?;

    let (mut messages, system_prompt) = convert_messages(payload.get("messages"));

    if let Some(assistant_prefill) = payload
        .get("assistant_prefill")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": assistant_prefill,
                }
            ]
        }));
    }

    if messages.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": "",
                }
            ]
        }));
    }

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));
    request.insert("messages".to_string(), Value::Array(messages));
    request.insert(
        "max_tokens".to_string(),
        payload
            .get("max_tokens")
            .cloned()
            .or_else(|| payload.get("max_completion_tokens").cloned())
            .unwrap_or_else(|| Value::Number(serde_json::Number::from(1024))),
    );

    for key in ["temperature", "top_p", "top_k", "stream"] {
        insert_if_present(&mut request, payload, key);
    }

    if let Some(stop) = payload.get("stop").filter(|value| value.is_array()) {
        request.insert("stop_sequences".to_string(), stop.clone());
    }

    let use_system_prompt = payload
        .get("use_sysprompt")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if use_system_prompt && !system_prompt.is_empty() {
        request.insert("system".to_string(), Value::String(system_prompt));
    }

    let mut claude_tools = payload
        .get("tools")
        .map(map_openai_tools_to_claude)
        .unwrap_or_default();

    let mut forced_tool_choice: Option<Value> = None;
    if let Some(json_schema) = payload.get("json_schema").and_then(Value::as_object) {
        if let Some(schema_value) = json_schema
            .get("value")
            .cloned()
            .filter(|value| !value.is_null())
        {
            let schema_name = json_schema
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("response")
                .to_string();

            let mut schema_tool = Map::new();
            schema_tool.insert("name".to_string(), Value::String(schema_name.clone()));
            schema_tool.insert(
                "description".to_string(),
                Value::String("Well-formed JSON object".to_string()),
            );
            schema_tool.insert("input_schema".to_string(), schema_value);
            claude_tools.push(Value::Object(schema_tool));

            forced_tool_choice = Some(json!({
                "type": "tool",
                "name": schema_name,
            }));
        }
    }

    if !claude_tools.is_empty() {
        request.insert("tools".to_string(), Value::Array(claude_tools));

        let tool_choice = forced_tool_choice.or_else(|| {
            payload
                .get("tool_choice")
                .and_then(map_tool_choice_to_claude)
        });
        if let Some(choice) = tool_choice {
            request.insert("tool_choice".to_string(), choice);
        }
    }

    Ok(request)
}

fn convert_messages(messages: Option<&Value>) -> (Vec<Value>, String) {
    let mut converted = Vec::new();
    let mut system_parts = Vec::new();

    let Some(messages) = messages else {
        return (converted, String::new());
    };

    if let Some(prompt) = messages.as_str() {
        converted.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": prompt }],
        }));
        return (converted, String::new());
    }

    let Some(entries) = messages.as_array() else {
        return (converted, String::new());
    };

    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_lowercase();

        let content_text = message_content_to_text(message.get("content"));

        match role.as_str() {
            "system" => {
                if !content_text.is_empty() {
                    system_parts.push(content_text);
                }
            }
            "assistant" => {
                let mut content_blocks = Vec::new();

                if !content_text.is_empty() {
                    content_blocks.push(json!({
                        "type": "text",
                        "text": content_text,
                    }));
                }

                if let Some(tool_calls) = message.get("tool_calls") {
                    content_blocks.extend(convert_openai_tool_calls_to_claude_blocks(tool_calls));
                }

                if !content_blocks.is_empty() {
                    converted.push(json!({
                        "role": "assistant",
                        "content": content_blocks,
                    }));
                }
            }
            "tool" => {
                if !content_text.is_empty() {
                    let tool_name = message
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("tool");

                    converted.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "text",
                            "text": format!("Tool `{tool_name}` result:\n{content_text}"),
                        }],
                    }));
                }
            }
            _ => {
                converted.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "text",
                        "text": content_text,
                    }],
                }));
            }
        }
    }

    (converted, system_parts.join("\n\n"))
}

fn convert_openai_tool_calls_to_claude_blocks(tool_calls: &Value) -> Vec<Value> {
    let Some(entries) = tool_calls.as_array() else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|entry| {
            let object = entry.as_object()?;
            let function = object.get("function")?.as_object()?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .map(|raw| {
                    serde_json::from_str::<Value>(raw)
                        .unwrap_or_else(|_| Value::String(raw.to_string()))
                })
                .unwrap_or_else(|| Value::Object(Map::new()));
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("tool_call");

            Some(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": arguments,
            }))
        })
        .collect()
}

fn map_openai_tools_to_claude(tools: &Value) -> Vec<Value> {
    let Some(entries) = tools.as_array() else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|tool| {
            let object = tool.as_object()?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return None;
            }

            let function = object.get("function")?.as_object()?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?
                .to_string();

            let mut mapped = Map::new();
            mapped.insert("name".to_string(), Value::String(name));
            if let Some(description) = function
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                mapped.insert(
                    "description".to_string(),
                    Value::String(description.to_string()),
                );
            }

            let input_schema = function
                .get("parameters")
                .cloned()
                .filter(|value| !value.is_null())
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
            mapped.insert("input_schema".to_string(), input_schema);

            Some(Value::Object(mapped))
        })
        .collect()
}

fn map_tool_choice_to_claude(value: &Value) -> Option<Value> {
    if let Some(choice) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match choice {
            "auto" => Some(json!({ "type": "auto" })),
            "required" => Some(json!({ "type": "any" })),
            "none" => None,
            _ => Some(json!({ "type": "auto" })),
        };
    }

    let object = value.as_object()?;
    if let Some(function_name) = object
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(json!({
            "type": "tool",
            "name": function_name,
        }));
    }

    object
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|raw| match raw {
            "tool" | "auto" | "any" => Some(json!({ "type": raw })),
            _ => None,
        })
}
