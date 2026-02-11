use serde_json::{json, Map, Value};

use crate::application::errors::ApplicationError;

use super::shared::{message_content_to_text, parse_data_url};

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    Ok((
        String::new(),
        Value::Object(build_makersuite_payload(&payload)?),
    ))
}

fn build_makersuite_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Gemini request is missing model".to_string())
        })?;

    let (contents, system_prompt) = convert_messages(payload.get("messages"));

    let mut generation_config = Map::new();
    generation_config.insert(
        "candidateCount".to_string(),
        Value::Number(serde_json::Number::from(1)),
    );

    if let Some(value) = payload.get("max_tokens").filter(|value| !value.is_null()) {
        generation_config.insert("maxOutputTokens".to_string(), value.clone());
    }

    for (source_key, target_key) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("top_k", "topK"),
        ("seed", "seed"),
    ] {
        if let Some(value) = payload.get(source_key).filter(|value| !value.is_null()) {
            generation_config.insert(target_key.to_string(), value.clone());
        }
    }

    if let Some(stop) = payload
        .get("stop")
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
    {
        generation_config.insert("stopSequences".to_string(), Value::Array(stop.clone()));
    }

    if let Some(json_schema) = payload.get("json_schema").and_then(Value::as_object) {
        if let Some(schema_value) = json_schema
            .get("value")
            .cloned()
            .filter(|value| !value.is_null())
        {
            generation_config.insert(
                "responseMimeType".to_string(),
                Value::String("application/json".to_string()),
            );
            generation_config.insert("responseSchema".to_string(), schema_value);
        }
    }

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));
    request.insert(
        "contents".to_string(),
        Value::Array(if contents.is_empty() {
            vec![json!({
                "role": "user",
                "parts": [{ "text": "" }],
            })]
        } else {
            contents
        }),
    );
    request.insert(
        "generationConfig".to_string(),
        Value::Object(generation_config),
    );

    let use_system_prompt = payload
        .get("use_sysprompt")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if use_system_prompt && !system_prompt.is_empty() {
        request.insert(
            "systemInstruction".to_string(),
            json!({
                "parts": [{ "text": system_prompt }],
            }),
        );
    }

    let declarations = payload
        .get("tools")
        .map(map_openai_tools_to_makersuite)
        .unwrap_or_default();
    if !declarations.is_empty() {
        request.insert(
            "tools".to_string(),
            Value::Array(vec![json!({ "functionDeclarations": declarations })]),
        );

        if let Some(tool_choice) = payload
            .get("tool_choice")
            .and_then(map_tool_choice_to_makersuite)
        {
            request.insert(
                "toolConfig".to_string(),
                json!({ "functionCallingConfig": tool_choice }),
            );
        }
    }

    Ok(request)
}

fn convert_messages(messages: Option<&Value>) -> (Vec<Value>, String) {
    let mut contents = Vec::new();
    let mut system_parts = Vec::new();

    let Some(messages) = messages else {
        return (contents, String::new());
    };

    if let Some(prompt) = messages.as_str() {
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": prompt }],
        }));
        return (contents, String::new());
    }

    let Some(entries) = messages.as_array() else {
        return (contents, String::new());
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

        if role == "system" {
            if !content_text.is_empty() {
                system_parts.push(content_text);
            }
            continue;
        }

        let mut parts = convert_message_content_to_parts(message.get("content"));

        if role == "assistant" {
            if let Some(tool_calls) = message.get("tool_calls") {
                parts.extend(convert_openai_tool_calls_to_parts(tool_calls));
            }
        }

        if role == "tool" {
            let name = message
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("tool");
            parts = vec![build_tool_response_part(name, content_text)];
        }

        if parts.is_empty() {
            parts.push(json!({ "text": "" }));
        }

        let target_role = if role == "assistant" { "model" } else { "user" };
        contents.push(json!({
            "role": target_role,
            "parts": parts,
        }));
    }

    (contents, system_parts.join("\n\n"))
}

fn convert_message_content_to_parts(content: Option<&Value>) -> Vec<Value> {
    let Some(content) = content else {
        return Vec::new();
    };

    match content {
        Value::String(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "text": text })]
            }
        }
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(text) => Some(json!({ "text": text })),
                Value::Object(object) => {
                    if let Some(text) = object
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        return Some(json!({ "text": text }));
                    }

                    if let Some(inline_data) =
                        object.get("inlineData").filter(|value| value.is_object())
                    {
                        return Some(json!({ "inlineData": inline_data.clone() }));
                    }

                    if let Some(inline_data) =
                        object.get("inline_data").filter(|value| value.is_object())
                    {
                        return Some(json!({ "inlineData": inline_data.clone() }));
                    }

                    if let Some(function_call) =
                        object.get("functionCall").filter(|value| value.is_object())
                    {
                        return Some(json!({ "functionCall": function_call.clone() }));
                    }

                    if object
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == "image_url")
                    {
                        let data_url = object
                            .get("image_url")
                            .and_then(Value::as_object)
                            .and_then(|entry| entry.get("url"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty());

                        if let Some(data_url) = data_url {
                            if let Some((mime_type, data)) = parse_data_url(data_url) {
                                return Some(json!({
                                    "inlineData": {
                                        "mimeType": mime_type,
                                        "data": data,
                                    }
                                }));
                            }

                            return Some(json!({ "text": data_url }));
                        }
                    }

                    None
                }
                _ => None,
            })
            .collect(),
        Value::Null => Vec::new(),
        other => vec![json!({ "text": other.to_string() })],
    }
}

fn convert_openai_tool_calls_to_parts(tool_calls: &Value) -> Vec<Value> {
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

            let args = function
                .get("arguments")
                .and_then(Value::as_str)
                .map(|raw| {
                    serde_json::from_str::<Value>(raw)
                        .unwrap_or_else(|_| Value::String(raw.to_string()))
                })
                .unwrap_or_else(|| Value::Object(Map::new()));

            Some(json!({
                "functionCall": {
                    "name": name,
                    "args": args,
                }
            }))
        })
        .collect()
}

fn build_tool_response_part(name: &str, content: String) -> Value {
    let parsed = serde_json::from_str::<Value>(&content).unwrap_or_else(|_| {
        json!({
            "content": content,
        })
    });

    json!({
        "functionResponse": {
            "name": name,
            "response": parsed,
        }
    })
}

fn map_openai_tools_to_makersuite(tools: &Value) -> Vec<Value> {
    let Some(entries) = tools.as_array() else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|entry| {
            let object = entry.as_object()?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return None;
            }

            let function = object.get("function")?.as_object()?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;

            let mut declaration = Map::new();
            declaration.insert("name".to_string(), Value::String(name.to_string()));

            if let Some(description) = function
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                declaration.insert(
                    "description".to_string(),
                    Value::String(description.to_string()),
                );
            }

            if let Some(parameters) = function.get("parameters").and_then(Value::as_object) {
                let mut parameters = parameters.clone();
                parameters.remove("$schema");
                declaration.insert("parameters".to_string(), Value::Object(parameters));
            }

            Some(Value::Object(declaration))
        })
        .collect()
}

fn map_tool_choice_to_makersuite(value: &Value) -> Option<Value> {
    if let Some(choice) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match choice {
            "none" => Some(json!({ "mode": "NONE" })),
            "required" => Some(json!({ "mode": "ANY" })),
            "auto" => Some(json!({ "mode": "AUTO" })),
            _ => None,
        };
    }

    let object = value.as_object()?;
    let function_name = object
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(function_name) = function_name {
        return Some(json!({
            "mode": "ANY",
            "allowedFunctionNames": [function_name],
        }));
    }

    None
}
