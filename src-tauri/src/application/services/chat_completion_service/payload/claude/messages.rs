use serde_json::Value;
use serde_json::json;

use crate::application::errors::ApplicationError;

use super::super::shared::{message_content_to_text, parse_data_url};
use super::super::tool_calls::{
    extract_openai_tool_calls, message_tool_call_id, message_tool_result_text,
};
use super::tools::convert_openai_tool_calls_to_claude_blocks;

const CLAUDE_EMPTY_TEXT_PLACEHOLDER: &str = "\u{200b}";

pub(super) fn convert_messages(
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
            let Some(message) = entries.get(start_index).and_then(Value::as_object) else {
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
                let content_blocks = if let Some(native_content) =
                    message_native_claude_content(message)
                {
                    native_content
                } else if !tool_calls.is_empty() {
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

fn message_native_claude_content(message: &serde_json::Map<String, Value>) -> Option<Vec<Value>> {
    message
        .get("native")?
        .get("claude")?
        .get("content")?
        .as_array()
        .cloned()
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

pub(super) fn move_assistant_images_to_next_user_message(messages: &mut Vec<Value>) {
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

pub(super) fn merge_consecutive_messages(messages: &mut Vec<Value>) {
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
