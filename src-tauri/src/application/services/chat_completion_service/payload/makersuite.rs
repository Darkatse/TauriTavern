use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::application::errors::ApplicationError;

use super::shared::{message_content_to_text, parse_data_url};
use super::tool_calls::{
    OpenAiToolCall, extract_openai_tool_calls, fallback_tool_name, message_tool_call_id,
    message_tool_name, message_tool_result_text, normalize_tool_result_payload,
};

const GOOGLE_FLASH_MAX_BUDGET: i64 = 24_576;
const GOOGLE_PRO_MAX_BUDGET: i64 = 32_768;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let endpoint = if stream {
        "/streamGenerateContent"
    } else {
        "/generateContent"
    };

    Ok((
        endpoint.to_string(),
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

    inject_google_thinking_config(payload, model, &mut generation_config);

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
    let mut tool_name_by_id: HashMap<String, String> = HashMap::new();

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
            let tool_calls = extract_openai_tool_calls(message.get("tool_calls"));
            if !tool_calls.is_empty() {
                for tool_call in &tool_calls {
                    tool_name_by_id.insert(tool_call.id.clone(), tool_call.name.clone());
                }
                parts.extend(convert_openai_tool_calls_to_parts(&tool_calls));
            }
        }

        if role == "tool" {
            let tool_call_id = message_tool_call_id(message);
            let name = message_tool_name(message)
                .or_else(|| {
                    tool_call_id
                        .as_ref()
                        .and_then(|id| tool_name_by_id.get(id))
                        .cloned()
                })
                .unwrap_or_else(|| fallback_tool_name().to_string());
            let content = message_tool_result_text(message);
            parts = vec![build_tool_response_part(&name, &content)];
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

fn convert_openai_tool_calls_to_parts(tool_calls: &[OpenAiToolCall]) -> Vec<Value> {
    tool_calls
        .iter()
        .map(|tool_call| {
            let mut part = json!({
                "functionCall": {
                    "name": tool_call.name,
                    "args": tool_call.arguments,
                }
            });

            if let Some(signature) = tool_call.signature.as_ref() {
                if let Some(part_object) = part.as_object_mut() {
                    part_object.insert(
                        "thoughtSignature".to_string(),
                        Value::String(signature.clone()),
                    );
                }
            }

            part
        })
        .collect()
}

fn build_tool_response_part(name: &str, content: &str) -> Value {
    json!({
        "functionResponse": {
            "name": name,
            "response": normalize_tool_result_payload(content),
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

fn inject_google_thinking_config(
    payload: &Map<String, Value>,
    model: &str,
    generation_config: &mut Map<String, Value>,
) {
    if !is_google_thinking_config_model(model) {
        return;
    }

    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reasoning_effort = payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let max_output_tokens = generation_config
        .get("maxOutputTokens")
        .and_then(value_to_i64)
        .unwrap_or(0);

    let mut thinking_config = Map::new();
    thinking_config.insert(
        "includeThoughts".to_string(),
        Value::Bool(include_reasoning),
    );

    if let Some(budget) = calculate_google_budget_tokens(max_output_tokens, reasoning_effort, model)
    {
        match budget {
            GoogleThinkingBudget::Tokens(tokens) => {
                thinking_config.insert(
                    "thinkingBudget".to_string(),
                    Value::Number(serde_json::Number::from(tokens)),
                );
            }
            GoogleThinkingBudget::Level(level) => {
                thinking_config.insert(
                    "thinkingLevel".to_string(),
                    Value::String(level.to_string()),
                );
            }
        }
    }

    generation_config.insert("thinkingConfig".to_string(), Value::Object(thinking_config));
}

fn is_google_thinking_config_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    let is_gemini_25 = (model.starts_with("gemini-2.5-flash")
        || model.starts_with("gemini-2.5-pro"))
        && !model.ends_with("-image")
        && !model.ends_with("-image-preview");
    let is_gemini_3 = model.starts_with("gemini-3-flash") || model.starts_with("gemini-3-pro");

    is_gemini_25 || is_gemini_3
}

enum GoogleThinkingBudget {
    Tokens(i64),
    Level(&'static str),
}

fn calculate_google_budget_tokens(
    max_tokens: i64,
    reasoning_effort: &str,
    model: &str,
) -> Option<GoogleThinkingBudget> {
    let model = model.trim().to_ascii_lowercase();
    let effort = reasoning_effort.trim().to_ascii_lowercase();
    let max_tokens = max_tokens.max(0);

    if model.contains("gemini-3-pro") {
        let level = match effort.as_str() {
            "auto" => return None,
            "min" | "low" | "medium" => "low",
            "high" | "max" => "high",
            _ => return None,
        };
        return Some(GoogleThinkingBudget::Level(level));
    }

    if model.contains("gemini-3-flash") {
        let level = match effort.as_str() {
            "auto" => return None,
            "min" => "minimal",
            "low" => "low",
            "medium" => "medium",
            "high" | "max" => "high",
            _ => return None,
        };
        return Some(GoogleThinkingBudget::Level(level));
    }

    if model.contains("flash-lite") {
        let tokens = match effort.as_str() {
            "auto" => return Some(GoogleThinkingBudget::Tokens(-1)),
            "min" => 0,
            "low" => max_tokens.saturating_mul(10) / 100,
            "medium" => max_tokens.saturating_mul(25) / 100,
            "high" => max_tokens.saturating_mul(50) / 100,
            "max" => max_tokens,
            _ => return None,
        };

        return Some(GoogleThinkingBudget::Tokens(
            tokens.clamp(512, GOOGLE_FLASH_MAX_BUDGET),
        ));
    }

    if model.contains("flash") {
        let tokens = match effort.as_str() {
            "auto" => return Some(GoogleThinkingBudget::Tokens(-1)),
            "min" => 0,
            "low" => max_tokens.saturating_mul(10) / 100,
            "medium" => max_tokens.saturating_mul(25) / 100,
            "high" => max_tokens.saturating_mul(50) / 100,
            "max" => max_tokens,
            _ => return None,
        };

        return Some(GoogleThinkingBudget::Tokens(
            tokens.clamp(0, GOOGLE_FLASH_MAX_BUDGET),
        ));
    }

    if model.contains("pro") {
        let tokens = match effort.as_str() {
            "auto" => return Some(GoogleThinkingBudget::Tokens(-1)),
            "min" => 128,
            "low" => max_tokens.saturating_mul(10) / 100,
            "medium" => max_tokens.saturating_mul(25) / 100,
            "high" => max_tokens.saturating_mul(50) / 100,
            "max" => max_tokens,
            _ => return None,
        };

        return Some(GoogleThinkingBudget::Tokens(
            tokens.clamp(128, GOOGLE_PRO_MAX_BUDGET),
        ));
    }

    None
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn makersuite_25_flash_sets_numeric_thinking_budget() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 4000,
            "reasoning_effort": "medium",
            "include_reasoning": true
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let config = body
            .get("generationConfig")
            .and_then(Value::as_object)
            .expect("generationConfig must be object");
        let thinking = config
            .get("thinkingConfig")
            .and_then(Value::as_object)
            .expect("thinkingConfig must be object");

        assert_eq!(
            thinking
                .get("thinkingBudget")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            1000
        );
        assert_eq!(
            thinking
                .get("includeThoughts")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            true
        );
    }

    #[test]
    fn makersuite_3_pro_sets_thinking_level() {
        let payload = json!({
            "model": "gemini-3-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8000,
            "reasoning_effort": "medium",
            "include_reasoning": false
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let config = body
            .get("generationConfig")
            .and_then(Value::as_object)
            .expect("generationConfig must be object");
        let thinking = config
            .get("thinkingConfig")
            .and_then(Value::as_object)
            .expect("thinkingConfig must be object");

        assert_eq!(
            thinking
                .get("thinkingLevel")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "low"
        );
        assert!(thinking.get("thinkingBudget").is_none());
    }

    #[test]
    fn makersuite_image_model_does_not_set_thinking_config() {
        let payload = json!({
            "model": "gemini-2.5-flash-image-preview",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 1024,
            "reasoning_effort": "high",
            "include_reasoning": true
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let config = body
            .get("generationConfig")
            .and_then(Value::as_object)
            .expect("generationConfig must be object");

        assert!(config.get("thinkingConfig").is_none());
    }

    #[test]
    fn makersuite_tool_result_uses_previous_tool_call_name() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "messages": [
                {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_weather",
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "arguments": "{\"city\":\"Paris\"}"
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_weather",
                    "content": "{\"temperature\":20}"
                }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let contents = body
            .get("contents")
            .and_then(Value::as_array)
            .expect("contents must be array");

        let model_part = contents
            .first()
            .and_then(Value::as_object)
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(Value::as_object)
            .and_then(|part| part.get("functionCall"))
            .and_then(Value::as_object)
            .expect("functionCall must exist");
        assert_eq!(
            model_part
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "weather"
        );

        let user_part = contents
            .get(1)
            .and_then(Value::as_object)
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(Value::as_object)
            .and_then(|part| part.get("functionResponse"))
            .and_then(Value::as_object)
            .expect("functionResponse must exist");
        assert_eq!(
            user_part
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "weather"
        );
        assert_eq!(
            user_part
                .get("response")
                .and_then(Value::as_object)
                .and_then(|response| response.get("temperature"))
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            20
        );
    }

    #[test]
    fn makersuite_tool_call_signature_maps_to_thought_signature() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_weather",
                    "type": "function",
                    "function": {
                        "name": "weather",
                        "arguments": "{}"
                    },
                    "signature": "sig_1"
                }]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let thought_signature = body
            .get("contents")
            .and_then(Value::as_array)
            .and_then(|contents| contents.first())
            .and_then(Value::as_object)
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(Value::as_object)
            .and_then(|part| part.get("thoughtSignature"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        assert_eq!(thought_signature, "sig_1");
    }
}
