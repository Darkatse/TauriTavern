use serde_json::{Map, Number, Value, json};

use crate::application::errors::ApplicationError;

use super::shared::{insert_if_present, message_content_to_text, parse_data_url};
use super::tool_calls::{
    OpenAiToolCall, extract_openai_tool_calls, message_tool_call_id, message_tool_result_text,
};

const CLAUDE_THINKING_MIN_TOKENS: i64 = 1024;
const CLAUDE_THINKING_NON_STREAM_CAP: i64 = 21_333;
const CLAUDE_EMPTY_TEXT_PLACEHOLDER: &str = "\u{200b}";

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

    let use_system_prompt = payload
        .get("use_sysprompt")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let use_tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
        || payload
            .get("json_schema")
            .and_then(Value::as_object)
            .and_then(|schema| schema.get("value"))
            .is_some_and(|value| !value.is_null());

    let (mut messages, system_prompt) =
        convert_messages(payload.get("messages"), use_system_prompt, use_tools)?;

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

    move_assistant_images_to_next_user_message(&mut messages);
    merge_consecutive_messages(&mut messages);

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

    let mut max_tokens = payload
        .get("max_tokens")
        .or_else(|| payload.get("max_completion_tokens"))
        .and_then(value_to_i64)
        .unwrap_or(CLAUDE_THINKING_MIN_TOKENS);
    max_tokens = max_tokens.max(1);
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));

    for key in ["temperature", "top_p", "top_k", "stream"] {
        insert_if_present(&mut request, payload, key);
    }

    if request.contains_key("temperature")
        && request.contains_key("top_p")
        && !claude_allows_temperature_and_top_p(model)
    {
        request.remove("top_p");
    }

    if let Some(stop) = payload.get("stop").filter(|value| value.is_array()) {
        request.insert("stop_sequences".to_string(), stop.clone());
    }

    if use_system_prompt && !system_prompt.is_empty() {
        request.insert("system".to_string(), Value::Array(system_prompt));
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

    let mut should_convert_prefill_to_user = requires_claude_prefill_role_fix(model);
    if supports_claude_thinking(model) {
        let reasoning_effort = payload.get("reasoning_effort").and_then(Value::as_str);
        if let Some(budget_tokens) =
            calculate_claude_budget_tokens(max_tokens, reasoning_effort, stream)
        {
            if max_tokens <= CLAUDE_THINKING_MIN_TOKENS {
                max_tokens += CLAUDE_THINKING_MIN_TOKENS;
            }

            request.insert(
                "thinking".to_string(),
                json!({
                    "type": "enabled",
                    "budget_tokens": budget_tokens,
                }),
            );
            request.remove("temperature");
            request.remove("top_p");
            request.remove("top_k");
            should_convert_prefill_to_user = true;
        }
    }

    if should_convert_prefill_to_user {
        convert_thinking_prefill_to_user(&mut messages);
        merge_consecutive_messages(&mut messages);
    }

    request.insert("messages".to_string(), Value::Array(messages));
    request.insert(
        "max_tokens".to_string(),
        Value::Number(Number::from(max_tokens)),
    );

    Ok(request)
}

fn convert_messages(
    messages: Option<&Value>,
    use_system_prompt: bool,
    use_tools: bool,
) -> Result<(Vec<Value>, Vec<Value>), ApplicationError> {
    let mut converted = Vec::new();
    let mut system_parts: Vec<Value> = Vec::new();

    let Some(messages) = messages else {
        return Ok((converted, system_parts));
    };

    if let Some(prompt) = messages.as_str() {
        converted.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": prompt }],
        }));
        return Ok((converted, system_parts));
    }

    let Some(entries) = messages.as_array() else {
        return Ok((converted, system_parts));
    };

    let mut start_index = 0_usize;
    if use_system_prompt {
        while start_index < entries.len() {
            let Some(message) = entries
                .get(start_index)
                .and_then(Value::as_object)
            else {
                start_index += 1;
                continue;
            };

            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user")
                .trim()
                .to_lowercase();
            if role != "system" {
                break;
            }

            let content_text = message_content_to_text(message.get("content"));
            if !content_text.is_empty() {
                system_parts.push(json!({
                    "type": "text",
                    "text": content_text,
                }));
            }

            start_index += 1;
        }
    }

    for entry in entries.iter().skip(start_index) {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_lowercase();

        let name = message
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        match role.as_str() {
            "assistant" => {
                let tool_calls = extract_openai_tool_calls(message.get("tool_calls"));
                let content_blocks = if !tool_calls.is_empty() {
                    if use_tools {
                        convert_openai_tool_calls_to_claude_blocks(&tool_calls)
                    } else {
                        tool_calls
                            .iter()
                            .map(|call| normalize_claude_text_block(&call.arguments.to_string()))
                            .collect()
                    }
                } else {
                    convert_message_content_to_claude_blocks(message.get("content"), name)?
                };

                if !content_blocks.is_empty() {
                    converted.push(json!({
                        "role": "assistant",
                        "content": content_blocks,
                    }));
                }
            }
            "tool" => {
                if !use_tools {
                    let result_text = message_tool_result_text(message);
                    converted.push(json!({
                        "role": "user",
                        "content": [normalize_claude_text_block(&result_text)],
                    }));
                } else if let Some(tool_use_id) = message_tool_call_id(message) {
                    converted.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": message_tool_result_text(message),
                        }],
                    }));
                } else {
                    let blocks =
                        convert_message_content_to_claude_blocks(message.get("content"), name)?;
                    let blocks = if blocks.is_empty() {
                        vec![normalize_claude_text_block("")]
                    } else {
                        blocks
                    };

                    converted.push(json!({
                        "role": "user",
                        "content": blocks,
                    }));
                }
            }
            _ => {
                let blocks =
                    convert_message_content_to_claude_blocks(message.get("content"), name)?;
                let blocks = if blocks.is_empty() {
                    vec![normalize_claude_text_block("")]
                } else {
                    blocks
                };

                converted.push(json!({
                    "role": "user",
                    "content": blocks,
                }));
            }
        }
    }

    Ok((converted, system_parts))
}

fn prefix_name(text: &str, name: Option<&str>) -> String {
    let Some(name) = name else {
        return text.to_string();
    };

    let name = name.trim();
    if name.is_empty() {
        return text.to_string();
    }

    let prefix = format!("{name}: ");
    if text.starts_with(&prefix) {
        text.to_string()
    } else {
        format!("{prefix}{text}")
    }
}

fn normalize_claude_text_block(text: &str) -> Value {
    let normalized = if text.is_empty() {
        CLAUDE_EMPTY_TEXT_PLACEHOLDER.to_string()
    } else {
        text.to_string()
    };

    json!({
        "type": "text",
        "text": normalized,
    })
}

fn convert_message_content_to_claude_blocks(
    content: Option<&Value>,
    name: Option<&str>,
) -> Result<Vec<Value>, ApplicationError> {
    let blocks = match content {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(text)) => vec![normalize_claude_text_block(&prefix_name(text, name))],
        Some(Value::Array(parts)) => {
            let mut blocks = Vec::with_capacity(parts.len());

            for part in parts {
                match part {
                    Value::String(fragment) => {
                        blocks.push(normalize_claude_text_block(&prefix_name(fragment, name)));
                    }
                    Value::Object(object) => match object.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let text = object
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            blocks.push(normalize_claude_text_block(&prefix_name(text, name)));
                        }
                        Some("image_url") => {
                            let data_url = object
                                .get("image_url")
                                .and_then(Value::as_object)
                                .and_then(|image_url| image_url.get("url"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .ok_or_else(|| {
                                    ApplicationError::ValidationError(
                                        "Claude image_url block is missing url".to_string(),
                                    )
                                })?;

                            let Some((mime_type, data)) = parse_data_url(data_url) else {
                                return Err(ApplicationError::ValidationError(
                                    "Claude expects image_url as a data URL".to_string(),
                                ));
                            };

                            blocks.push(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": mime_type,
                                    "data": data,
                                },
                            }));
                        }
                        _ => blocks.push(part.clone()),
                    },
                    _ => {}
                }
            }

            if blocks.is_empty() {
                blocks.push(normalize_claude_text_block(""));
            }

            blocks
        }
        Some(other) => vec![normalize_claude_text_block(&other.to_string())],
    };

    Ok(blocks)
}

fn is_claude_image_block(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|entry| entry == "image")
}

fn move_assistant_images_to_next_user_message(messages: &mut Vec<Value>) {
    let mut index = 0_usize;
    while index < messages.len() {
        let images: Vec<Value>;
        let remove_assistant: bool;

        {
            let mut collected_images = Vec::new();
            let Some(message_object) = messages.get_mut(index).and_then(Value::as_object_mut)
            else {
                index += 1;
                continue;
            };

            let role = message_object
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if role != "assistant" {
                index += 1;
                continue;
            }

            let Some(content) = message_object
                .get_mut("content")
                .and_then(Value::as_array_mut)
            else {
                index += 1;
                continue;
            };

            for block in content.iter() {
                if is_claude_image_block(block) {
                    collected_images.push(block.clone());
                }
            }

            if collected_images.is_empty() {
                index += 1;
                continue;
            }

            content.retain(|block| !is_claude_image_block(block));
            remove_assistant = content.is_empty();
            images = collected_images;
        }

        let mut target_index = index + 1;
        while target_index < messages.len() {
            let role = messages
                .get(target_index)
                .and_then(Value::as_object)
                .and_then(|object| object.get("role"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if role == "user" {
                break;
            }
            target_index += 1;
        }

        if target_index >= messages.len() {
            messages.insert(
                index + 1,
                json!({
                    "role": "user",
                    "content": [],
                }),
            );
            target_index = index + 1;
        }

        let Some(target_object) = messages
            .get_mut(target_index)
            .and_then(Value::as_object_mut)
        else {
            index += 1;
            continue;
        };

        let entry = target_object
            .entry("content".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(target_blocks) = entry.as_array_mut() {
            target_blocks.extend(images);
        }

        if remove_assistant {
            messages.remove(index);
            continue;
        }

        index += 1;
    }
}

fn merge_consecutive_messages(messages: &mut Vec<Value>) {
    let mut merged: Vec<Value> = Vec::with_capacity(messages.len());

    for message in std::mem::take(messages) {
        let role = message
            .as_object()
            .and_then(|object| object.get("role"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let can_merge = merged
            .last()
            .and_then(Value::as_object)
            .and_then(|object| object.get("role"))
            .and_then(Value::as_str)
            .is_some_and(|previous| previous == role);

        if !can_merge {
            merged.push(message);
            continue;
        }

        let Some(next_blocks) = message
            .as_object()
            .and_then(|object| object.get("content"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        let Some(last_object) = merged.last_mut().and_then(Value::as_object_mut) else {
            continue;
        };

        let entry = last_object
            .entry("content".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(last_blocks) = entry.as_array_mut() else {
            continue;
        };

        last_blocks.extend(next_blocks.iter().cloned());
    }

    *messages = merged;
}

fn convert_openai_tool_calls_to_claude_blocks(tool_calls: &[OpenAiToolCall]) -> Vec<Value> {
    tool_calls
        .iter()
        .map(|tool_call| {
            json!({
                "type": "tool_use",
                "id": tool_call.id,
                "name": tool_call.name,
                "input": tool_call.arguments,
            })
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

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

fn supports_claude_thinking(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    [
        "claude-3-7",
        "claude-opus-4",
        "claude-sonnet-4",
        "claude-haiku-4-5",
        "claude-opus-4-5",
        "claude-opus-4-6",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
}

fn claude_allows_temperature_and_top_p(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    [
        "claude-3",
        "claude-opus-4-0",
        "claude-opus-4-1",
        "claude-opus-4-20250514",
        "claude-sonnet-4-0",
        "claude-sonnet-4-20250514",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
}

fn requires_claude_prefill_role_fix(model: &str) -> bool {
    model
        .trim()
        .to_ascii_lowercase()
        .starts_with("claude-opus-4-6")
}

fn calculate_claude_budget_tokens(
    max_tokens: i64,
    reasoning_effort: Option<&str>,
    stream: bool,
) -> Option<i64> {
    let max_tokens = max_tokens.max(0);
    let effort = reasoning_effort
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "auto".to_string());

    let mut budget_tokens = match effort.as_str() {
        "auto" => return None,
        "min" => CLAUDE_THINKING_MIN_TOKENS,
        "low" => max_tokens.saturating_mul(10) / 100,
        "medium" => max_tokens.saturating_mul(25) / 100,
        "high" => max_tokens.saturating_mul(50) / 100,
        "max" => max_tokens.saturating_mul(95) / 100,
        _ => return None,
    };

    budget_tokens = budget_tokens.max(CLAUDE_THINKING_MIN_TOKENS);
    if !stream {
        budget_tokens = budget_tokens.min(CLAUDE_THINKING_NON_STREAM_CAP);
    }

    Some(budget_tokens)
}

fn convert_thinking_prefill_to_user(messages: &mut [Value]) {
    let Some(last_message) = messages.last_mut().and_then(Value::as_object_mut) else {
        return;
    };

    if last_message.get("role").and_then(Value::as_str) == Some("assistant") {
        last_message.insert("role".to_string(), Value::String("user".to_string()));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn claude_thinking_budget_updates_request_shape() {
        let payload = json!({
            "model": "claude-sonnet-4-5",
            "messages": [{"role": "user", "content": "hello"}],
            "assistant_prefill": "prefill",
            "max_tokens": 1000,
            "reasoning_effort": "medium",
            "temperature": 0.7,
            "top_p": 0.9,
            "top_k": 40,
            "stream": false,
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert_eq!(
            body.get("max_tokens")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            2024
        );
        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("budget_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            1024
        );
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("top_k").is_none());

        let last_role = body
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(last_role, "user");
    }

    #[test]
    fn claude_opus_4_6_prefill_is_converted_to_user_without_thinking() {
        let payload = json!({
            "model": "claude-opus-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "assistant_prefill": "prefill",
            "max_tokens": 1000,
            "reasoning_effort": "auto",
            "stream": false
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("thinking").is_none());

        let last_role = body
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(last_role, "user");
    }

    #[test]
    fn claude_use_sysprompt_collects_only_leading_system_messages() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "use_sysprompt": true,
            "messages": [
                {"role": "system", "content": "s1"},
                {"role": "system", "content": "s2"},
                {"role": "user", "content": "u1"},
                {"role": "system", "content": "late system"},
                {"role": "user", "content": "u2"}
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        let system = body
            .get("system")
            .and_then(Value::as_array)
            .expect("system must be array");
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["text"].as_str().unwrap_or_default(), "s1");
        assert_eq!(system[1]["text"].as_str().unwrap_or_default(), "s2");

        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");
        let joined = messages
            .iter()
            .filter_map(|message| message.get("content").and_then(Value::as_array))
            .flat_map(|parts| parts.iter())
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("late system"));
    }

    #[test]
    fn claude_system_messages_become_user_when_use_sysprompt_false() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "use_sysprompt": false,
            "messages": [
                {"role": "system", "content": "s1"},
                {"role": "user", "content": "u1"}
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("system").is_none());

        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");
        let joined = messages
            .iter()
            .filter_map(|message| message.get("content").and_then(Value::as_array))
            .flat_map(|parts| parts.iter())
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("s1"));
        assert!(joined.contains("u1"));
    }

    #[test]
    fn claude_tool_calls_and_results_are_structured() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
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
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "weather",
                    "description": "get weather",
                    "parameters": { "type": "object", "properties": {} }
                }
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        let assistant_blocks = messages
            .first()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .expect("assistant content must be array");
        assert_eq!(
            assistant_blocks[0]["type"].as_str().unwrap_or_default(),
            "tool_use"
        );
        assert_eq!(
            assistant_blocks[0]["id"].as_str().unwrap_or_default(),
            "call_weather"
        );
        assert_eq!(
            assistant_blocks[0]["name"].as_str().unwrap_or_default(),
            "weather"
        );

        let tool_result_block = messages
            .get(1)
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(Value::as_object)
            .expect("tool result block must be object");
        assert_eq!(
            tool_result_block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "tool_result"
        );
        assert_eq!(
            tool_result_block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "call_weather"
        );
    }

    #[test]
    fn claude_tool_calls_are_text_when_tools_disabled() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
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
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        let assistant_blocks = messages
            .first()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .expect("assistant content must be array");
        assert_eq!(
            assistant_blocks[0]["type"].as_str().unwrap_or_default(),
            "text"
        );

        let tool_blocks = messages
            .get(1)
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .expect("tool content must be array");
        assert_eq!(
            tool_blocks[0]["type"].as_str().unwrap_or_default(),
            "text"
        );
    }

    #[test]
    fn claude_converts_openai_image_url_blocks() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "describe" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                ]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        let content = messages[0]
            .get("content")
            .and_then(Value::as_array)
            .expect("message content must be array");

        assert_eq!(content[0]["type"].as_str().unwrap_or_default(), "text");
        assert_eq!(content[0]["text"].as_str().unwrap_or_default(), "describe");

        assert_eq!(content[1]["type"].as_str().unwrap_or_default(), "image");
        assert_eq!(
            content[1]["source"]["type"].as_str().unwrap_or_default(),
            "base64"
        );
        assert_eq!(
            content[1]["source"]["media_type"]
                .as_str()
                .unwrap_or_default(),
            "image/png"
        );
        assert_eq!(
            content[1]["source"]["data"].as_str().unwrap_or_default(),
            "AAAA"
        );
    }

    #[test]
    fn claude_moves_images_out_of_assistant_messages() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "here" },
                        { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                    ]
                },
                { "role": "user", "content": "ok" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        let assistant_content = messages[0]
            .get("content")
            .and_then(Value::as_array)
            .expect("assistant content must be array");
        assert!(
            !assistant_content
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("image"))
        );

        let user_content = messages[1]
            .get("content")
            .and_then(Value::as_array)
            .expect("user content must be array");
        assert!(
            user_content
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("image"))
        );
    }

    #[test]
    fn claude_4_5_models_drop_top_p_when_temperature_is_present() {
        let payload = json!({
            "model": "claude-sonnet-4-5",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7,
            "top_p": 0.9
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("temperature").is_some());
        assert!(body.get("top_p").is_none());
    }

    #[test]
    fn claude_3_models_keep_top_p_with_temperature() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7,
            "top_p": 0.9
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("temperature").is_some());
        assert!(body.get("top_p").is_some());
    }
}
