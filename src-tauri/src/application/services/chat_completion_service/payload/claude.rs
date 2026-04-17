use serde_json::{Map, Number, Value, json};

use crate::application::errors::ApplicationError;

use super::shared::{insert_if_present, message_content_to_text, parse_data_url};
use super::tool_calls::{
    OpenAiToolCall, extract_openai_tool_calls, message_tool_call_id, message_tool_result_text,
};

const CLAUDE_THINKING_MIN_TOKENS: i64 = 1024;
const CLAUDE_THINKING_NON_STREAM_CAP: i64 = 21_333;
const CLAUDE_EMPTY_TEXT_PLACEHOLDER: &str = "\u{200b}";
const CLAUDE_DEFAULT_TEMPERATURE: f64 = 1.0;
const CLAUDE_DEFAULT_TOP_P: f64 = 1.0;
const CLAUDE_DEFAULT_TOP_K: f64 = 0.0;
const CLAUDE_FULL_SAMPLING_EXACT_MODELS: &[&str] = &["claude-opus-4", "claude-sonnet-4"];
const CLAUDE_FULL_SAMPLING_MODEL_PREFIXES: &[&str] = &[
    "claude-3-7",
    "claude-3-5",
    "claude-3-opus",
    "claude-3-sonnet",
    "claude-3-haiku",
    "claude-2",
    "claude-instant",
];
const CLAUDE_LIMITED_SAMPLING_MODEL_PREFIXES: &[&str] = &[
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
];
const CLAUDE_MANUAL_ONLY_THINKING_EXACT_MODELS: &[&str] = &["claude-opus-4", "claude-sonnet-4"];
const CLAUDE_MANUAL_ONLY_THINKING_MODEL_PREFIXES: &[&str] = &[
    "claude-3-7",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
];
const CLAUDE_MANUAL_OR_ADAPTIVE_THINKING_MODEL_PREFIXES: &[&str] =
    &["claude-opus-4-6", "claude-sonnet-4-6"];
const CLAUDE_ADAPTIVE_ONLY_THINKING_MODEL_PREFIXES: &[&str] = &["claude-opus-4-7"];
const CLAUDE_OUTPUT_EFFORT_MODEL_PREFIXES: &[&str] = &[
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-opus-4-5",
];
const CLAUDE_ASSISTANT_PREFILL_EXACT_MODELS: &[&str] = &["claude-opus-4", "claude-sonnet-4"];
const CLAUDE_ASSISTANT_PREFILL_MODEL_PREFIXES: &[&str] = &[
    "claude-3-7",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
    "claude-3-5",
    "claude-3-opus",
    "claude-3-sonnet",
    "claude-3-haiku",
    "claude-2",
    "claude-instant",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeSamplingMode {
    Full,
    TemperatureOrTopP,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeThinkingMode {
    Unsupported,
    ManualOnly,
    ManualOrAdaptive,
    AdaptiveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeRequestThinkingMode {
    Enabled,
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeReasoningEffort {
    Min,
    Low,
    Medium,
    High,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaudeModelContract {
    sampling: ClaudeSamplingMode,
    thinking: ClaudeThinkingMode,
    supports_output_effort: bool,
    supports_assistant_prefill: bool,
}

impl ClaudeModelContract {
    fn resolve(model: &str) -> Self {
        let model = model.trim().to_ascii_lowercase();

        Self {
            sampling: resolve_claude_sampling_mode(&model),
            thinking: resolve_claude_thinking_mode(&model),
            supports_output_effort: matches_claude_model(
                &model,
                &[],
                CLAUDE_OUTPUT_EFFORT_MODEL_PREFIXES,
            ),
            supports_assistant_prefill: matches_claude_model(
                &model,
                CLAUDE_ASSISTANT_PREFILL_EXACT_MODELS,
                CLAUDE_ASSISTANT_PREFILL_MODEL_PREFIXES,
            ),
        }
    }
}

fn resolve_claude_sampling_mode(model: &str) -> ClaudeSamplingMode {
    if matches_claude_model(model, &[], CLAUDE_LIMITED_SAMPLING_MODEL_PREFIXES) {
        ClaudeSamplingMode::TemperatureOrTopP
    } else if matches_claude_model(
        model,
        CLAUDE_FULL_SAMPLING_EXACT_MODELS,
        CLAUDE_FULL_SAMPLING_MODEL_PREFIXES,
    ) {
        ClaudeSamplingMode::Full
    } else {
        ClaudeSamplingMode::None
    }
}

fn resolve_claude_thinking_mode(model: &str) -> ClaudeThinkingMode {
    if matches_claude_model(model, &[], CLAUDE_ADAPTIVE_ONLY_THINKING_MODEL_PREFIXES) {
        ClaudeThinkingMode::AdaptiveOnly
    } else if matches_claude_model(
        model,
        &[],
        CLAUDE_MANUAL_OR_ADAPTIVE_THINKING_MODEL_PREFIXES,
    ) {
        ClaudeThinkingMode::ManualOrAdaptive
    } else if matches_claude_model(
        model,
        CLAUDE_MANUAL_ONLY_THINKING_EXACT_MODELS,
        CLAUDE_MANUAL_ONLY_THINKING_MODEL_PREFIXES,
    ) {
        ClaudeThinkingMode::ManualOnly
    } else {
        ClaudeThinkingMode::Unsupported
    }
}

impl ClaudeReasoningEffort {
    fn parse(value: Option<&Value>) -> Result<Option<Self>, ApplicationError> {
        let Some(value) = value.and_then(Value::as_str) else {
            return Ok(None);
        };

        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("auto") {
            return Ok(None);
        }

        match value.to_ascii_lowercase().as_str() {
            "min" => Ok(Some(Self::Min)),
            "low" => Ok(Some(Self::Low)),
            "medium" => Ok(Some(Self::Medium)),
            "high" => Ok(Some(Self::High)),
            "max" => Ok(Some(Self::Max)),
            other => Err(ApplicationError::ValidationError(format!(
                "Unsupported Claude reasoning_effort: {other}"
            ))),
        }
    }

    fn calculate_budget_tokens(self, max_tokens: i64, stream: bool) -> i64 {
        let max_tokens = max_tokens.max(0);
        let mut budget_tokens = match self {
            Self::Min => CLAUDE_THINKING_MIN_TOKENS,
            Self::Low => max_tokens.saturating_mul(10) / 100,
            Self::Medium => max_tokens.saturating_mul(25) / 100,
            Self::High => max_tokens.saturating_mul(50) / 100,
            Self::Max => max_tokens.saturating_mul(95) / 100,
        };

        budget_tokens = budget_tokens.max(CLAUDE_THINKING_MIN_TOKENS);
        if !stream {
            budget_tokens = budget_tokens.min(CLAUDE_THINKING_NON_STREAM_CAP);
        }

        budget_tokens
    }

    fn as_adaptive_effort(self) -> &'static str {
        match self {
            Self::Min | Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let request = Value::Object(build_claude_payload(&payload)?);
    validate_request(&request)?;

    Ok(("/messages".to_string(), request))
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
    let contract = ClaudeModelContract::resolve(model);
    let reasoning_effort = ClaudeReasoningEffort::parse(payload.get("reasoning_effort"))?;

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

    let assistant_prefill = payload
        .get("assistant_prefill")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if assistant_prefill.is_some() && reasoning_effort.is_some() {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support assistant_prefill with reasoning_effort"
        )));
    }
    if assistant_prefill.is_some() && !contract.supports_assistant_prefill {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support assistant_prefill"
        )));
    }
    if let Some(assistant_prefill) = assistant_prefill {
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
    insert_if_present(&mut request, payload, "stream");
    insert_claude_sampling_params(&mut request, payload, contract.sampling, model)?;

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

    match contract.thinking {
        ClaudeThinkingMode::Unsupported => {
            if reasoning_effort.is_some() {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` does not support reasoning_effort"
                )));
            }
        }
        ClaudeThinkingMode::ManualOnly => {
            if let Some(reasoning_effort) = reasoning_effort {
                let budget_tokens = reasoning_effort.calculate_budget_tokens(max_tokens, stream);
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
            }
        }
        ClaudeThinkingMode::ManualOrAdaptive | ClaudeThinkingMode::AdaptiveOnly => {
            if let Some(reasoning_effort) = reasoning_effort {
                request.insert(
                    "thinking".to_string(),
                    build_claude_adaptive_thinking(payload),
                );
                if contract.supports_output_effort {
                    request.insert(
                        "output_config".to_string(),
                        json!({
                            "effort": reasoning_effort.as_adaptive_effort(),
                        }),
                    );
                }
            }
        }
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

fn value_to_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_u64().map(|number| number as f64))
}

pub(super) fn validate_request(payload: &Value) -> Result<(), ApplicationError> {
    let request = payload.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "Claude request payload must be a JSON object".to_string(),
        )
    })?;
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Claude request is missing model".to_string())
        })?;
    let contract = ClaudeModelContract::resolve(model);
    let thinking_mode = validate_claude_thinking_request(request, contract, model)?;
    validate_claude_output_config(request, contract, model)?;
    validate_claude_sampling_request(request, contract.sampling, thinking_mode, model)?;

    Ok(())
}

fn matches_claude_model(model: &str, exact_models: &[&str], prefixes: &[&str]) -> bool {
    exact_models.contains(&model) || prefixes.iter().any(|prefix| model.starts_with(prefix))
}

fn insert_claude_sampling_params(
    request: &mut Map<String, Value>,
    payload: &Map<String, Value>,
    sampling: ClaudeSamplingMode,
    model: &str,
) -> Result<(), ApplicationError> {
    let has_non_default_temperature = has_non_default_temperature(payload);
    let has_non_default_top_p = has_non_default_top_p(payload);
    let has_non_default_top_k = has_non_default_top_k(payload);

    match sampling {
        ClaudeSamplingMode::Full => {
            if has_non_default_temperature {
                insert_if_present(request, payload, "temperature");
            }
            if has_non_default_top_p {
                insert_if_present(request, payload, "top_p");
            }
            if has_non_default_top_k {
                insert_if_present(request, payload, "top_k");
            }
        }
        ClaudeSamplingMode::TemperatureOrTopP => {
            if has_non_default_temperature && has_non_default_top_p {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` accepts either temperature or top_p, not both"
                )));
            }

            if has_non_default_temperature {
                insert_if_present(request, payload, "temperature");
            }
            if has_non_default_top_p {
                insert_if_present(request, payload, "top_p");
            }
            if has_non_default_top_k {
                insert_if_present(request, payload, "top_k");
            }
        }
        ClaudeSamplingMode::None => {
            let disallowed = collect_non_default_sampling_params(payload);
            if !disallowed.is_empty() {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` does not support non-default sampling parameters: {}",
                    disallowed.join(", ")
                )));
            }
        }
    }

    Ok(())
}

fn build_claude_adaptive_thinking(payload: &Map<String, Value>) -> Value {
    let display = if payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "summarized"
    } else {
        "omitted"
    };

    json!({
        "type": "adaptive",
        "display": display,
    })
}

fn validate_claude_thinking_request(
    request: &Map<String, Value>,
    contract: ClaudeModelContract,
    model: &str,
) -> Result<Option<ClaudeRequestThinkingMode>, ApplicationError> {
    let Some(thinking) = request.get("thinking") else {
        return Ok(None);
    };

    let thinking = thinking.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "Claude model `{model}` expects `thinking` to be an object"
        ))
    })?;
    let thinking_type = thinking
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Claude model `{model}` requires `thinking.type`"
            ))
        })?;

    let thinking_mode = match thinking_type {
        "enabled" => {
            if thinking
                .get("budget_tokens")
                .and_then(value_to_i64)
                .is_none()
            {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` requires `thinking.budget_tokens` for legacy thinking"
                )));
            }
            ClaudeRequestThinkingMode::Enabled
        }
        "adaptive" => {
            if thinking.get("budget_tokens").is_some() {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` does not allow `thinking.budget_tokens` with adaptive thinking"
                )));
            }

            if let Some(display) = thinking
                .get("display")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if !matches!(display, "summarized" | "omitted") {
                    return Err(ApplicationError::ValidationError(format!(
                        "Unsupported Claude adaptive thinking display: {display}"
                    )));
                }
            }

            ClaudeRequestThinkingMode::Adaptive
        }
        other => {
            return Err(ApplicationError::ValidationError(format!(
                "Unsupported Claude thinking.type: {other}"
            )));
        }
    };

    match (contract.thinking, thinking_mode) {
        (ClaudeThinkingMode::Unsupported, _) => Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support thinking"
        ))),
        (ClaudeThinkingMode::ManualOnly, ClaudeRequestThinkingMode::Adaptive) => {
            Err(ApplicationError::ValidationError(format!(
                "Claude model `{model}` requires legacy thinking with budget_tokens"
            )))
        }
        (ClaudeThinkingMode::AdaptiveOnly, ClaudeRequestThinkingMode::Enabled) => {
            Err(ApplicationError::ValidationError(format!(
                "Claude model `{model}` requires adaptive thinking"
            )))
        }
        _ => Ok(Some(thinking_mode)),
    }
}

fn validate_claude_output_config(
    request: &Map<String, Value>,
    contract: ClaudeModelContract,
    model: &str,
) -> Result<(), ApplicationError> {
    let Some(output_config) = request.get("output_config") else {
        return Ok(());
    };

    let output_config = output_config.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "Claude model `{model}` expects `output_config` to be an object"
        ))
    })?;
    let Some(effort) = output_config.get("effort") else {
        return Ok(());
    };

    if !contract.supports_output_effort {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support `output_config.effort`"
        )));
    }

    let effort = effort
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Claude model `{model}` expects `output_config.effort` to be a non-empty string"
            ))
        })?;

    if !matches!(effort, "low" | "medium" | "high" | "max" | "xhigh") {
        return Err(ApplicationError::ValidationError(format!(
            "Unsupported Claude adaptive effort: {effort}"
        )));
    }

    Ok(())
}

fn validate_claude_sampling_request(
    request: &Map<String, Value>,
    sampling: ClaudeSamplingMode,
    thinking_mode: Option<ClaudeRequestThinkingMode>,
    model: &str,
) -> Result<(), ApplicationError> {
    let disallowed = collect_non_default_sampling_params(request);
    if thinking_mode == Some(ClaudeRequestThinkingMode::Enabled) && !disallowed.is_empty() {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not allow non-default sampling parameters with legacy thinking: {}",
            disallowed.join(", ")
        )));
    }

    match sampling {
        ClaudeSamplingMode::Full => Ok(()),
        ClaudeSamplingMode::TemperatureOrTopP => {
            if has_non_default_temperature(request) && has_non_default_top_p(request) {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` accepts either temperature or top_p, not both"
                )));
            }
            Ok(())
        }
        ClaudeSamplingMode::None => {
            if disallowed.is_empty() {
                Ok(())
            } else {
                Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` does not support non-default sampling parameters: {}",
                    disallowed.join(", ")
                )))
            }
        }
    }
}

fn collect_non_default_sampling_params(payload: &Map<String, Value>) -> Vec<&'static str> {
    let mut params = Vec::new();
    if has_non_default_temperature(payload) {
        params.push("temperature");
    }
    if has_non_default_top_p(payload) {
        params.push("top_p");
    }
    if has_non_default_top_k(payload) {
        params.push("top_k");
    }
    params
}

fn has_non_default_temperature(payload: &Map<String, Value>) -> bool {
    numeric_field_differs_from_default(payload, "temperature", CLAUDE_DEFAULT_TEMPERATURE)
}

fn has_non_default_top_p(payload: &Map<String, Value>) -> bool {
    numeric_field_differs_from_default(payload, "top_p", CLAUDE_DEFAULT_TOP_P)
}

fn has_non_default_top_k(payload: &Map<String, Value>) -> bool {
    numeric_field_differs_from_default(payload, "top_k", CLAUDE_DEFAULT_TOP_K)
}

fn numeric_field_differs_from_default(
    payload: &Map<String, Value>,
    key: &str,
    default: f64,
) -> bool {
    payload
        .get(key)
        .and_then(value_to_f64)
        .is_some_and(|value| (value - default).abs() > f64::EPSILON)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    fn claude_payload(model: &str) -> serde_json::Map<String, Value> {
        json!({
            "model": model,
            "messages": [{"role": "user", "content": "hello"}],
        })
        .as_object()
        .cloned()
        .expect("payload must be object")
    }

    #[test]
    fn claude_manual_reasoning_uses_legacy_thinking_and_clears_sampling() {
        let mut payload = claude_payload("claude-sonnet-4-5");
        payload.insert("max_tokens".to_string(), json!(1000));
        payload.insert("reasoning_effort".to_string(), json!("medium"));
        payload.insert("temperature".to_string(), json!(0.7));
        payload.insert("top_k".to_string(), json!(40));
        payload.insert("stream".to_string(), json!(false));

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
        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "enabled"
        );
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("top_k").is_none());
    }

    #[test]
    fn claude_rejects_assistant_prefill_for_models_that_removed_it() {
        for model in ["claude-opus-4-7", "claude-opus-4-6", "claude-sonnet-4-6"] {
            let mut payload = claude_payload(model);
            payload.insert("assistant_prefill".to_string(), json!("prefill"));

            let error = build(payload).expect_err("build should fail");
            let message = error.to_string();

            assert!(
                message.contains("does not support assistant_prefill"),
                "{model} should reject assistant_prefill, got: {message}"
            );
        }
    }

    #[test]
    fn claude_rejects_assistant_prefill_with_reasoning_effort() {
        let mut payload = claude_payload("claude-sonnet-4-5");
        payload.insert("assistant_prefill".to_string(), json!("prefill"));
        payload.insert("reasoning_effort".to_string(), json!("medium"));

        let error = build(payload).expect_err("build should fail");
        assert!(
            error
                .to_string()
                .contains("does not support assistant_prefill with reasoning_effort")
        );
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
        assert_eq!(tool_blocks[0]["type"].as_str().unwrap_or_default(), "text");
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
    fn claude_limited_sampling_models_reject_temperature_and_top_p_together() {
        let mut payload = claude_payload("claude-sonnet-4-5");
        payload.insert("temperature".to_string(), json!(0.7));
        payload.insert("top_p".to_string(), json!(0.9));

        let error = build(payload).expect_err("build should fail");
        assert!(
            error
                .to_string()
                .contains("accepts either temperature or top_p, not both")
        );
    }

    #[test]
    fn claude_full_sampling_models_keep_temperature_top_p_and_top_k() {
        let mut payload = claude_payload("claude-3-5-sonnet-latest");
        payload.insert("temperature".to_string(), json!(0.7));
        payload.insert("top_p".to_string(), json!(0.9));
        payload.insert("top_k".to_string(), json!(40));

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("temperature").is_some());
        assert!(body.get("top_p").is_some());
        assert!(body.get("top_k").is_some());
    }

    #[test]
    fn claude_sampling_free_models_reject_non_default_sampling_params() {
        let mut payload = claude_payload("claude-opus-4-7");
        payload.insert("temperature".to_string(), json!(0.7));

        let error = build(payload).expect_err("build should fail");
        assert!(
            error
                .to_string()
                .contains("does not support non-default sampling parameters: temperature")
        );
    }

    #[test]
    fn claude_sampling_free_models_ignore_default_sampling_params() {
        let mut payload = claude_payload("claude-opus-4-7");
        payload.insert("temperature".to_string(), json!(1.0));
        payload.insert("top_p".to_string(), json!(1.0));
        payload.insert("top_k".to_string(), json!(0));

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("top_k").is_none());
    }

    #[test]
    fn claude_future_models_do_not_inherit_sampling_or_prefill_support() {
        let mut sampling_payload = claude_payload("claude-opus-4-8");
        sampling_payload.insert("temperature".to_string(), json!(0.7));

        let sampling_error = build(sampling_payload).expect_err("build should fail");
        assert!(
            sampling_error
                .to_string()
                .contains("does not support non-default sampling parameters: temperature")
        );

        let mut prefill_payload = claude_payload("claude-opus-4-8");
        prefill_payload.insert("assistant_prefill".to_string(), json!("prefill"));

        let prefill_error = build(prefill_payload).expect_err("build should fail");
        assert!(
            prefill_error
                .to_string()
                .contains("does not support assistant_prefill")
        );
    }

    #[test]
    fn claude_future_models_do_not_inherit_legacy_or_adaptive_reasoning_support() {
        let mut payload = claude_payload("claude-opus-4-8");
        payload.insert("reasoning_effort".to_string(), json!("medium"));

        let error = build(payload).expect_err("build should fail");
        assert!(
            error
                .to_string()
                .contains("does not support reasoning_effort")
        );
    }

    #[test]
    fn claude_adaptive_reasoning_uses_adaptive_thinking_and_effort() {
        let mut payload = claude_payload("claude-opus-4-7");
        payload.insert("reasoning_effort".to_string(), json!("high"));
        payload.insert("include_reasoning".to_string(), json!(true));

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "adaptive"
        );
        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("display"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "summarized"
        );
        assert_eq!(
            body.get("output_config")
                .and_then(Value::as_object)
                .and_then(|config| config.get("effort"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "high"
        );
        assert!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("budget_tokens"))
                .is_none()
        );
    }

    #[test]
    fn claude_manual_or_adaptive_models_accept_legacy_thinking_overrides() {
        let request = json!({
            "model": "claude-opus-4-6",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
            "thinking": {
                "type": "enabled",
                "budget_tokens": 2048
            },
            "max_tokens": 4096
        });

        super::validate_request(&request).expect("request should be valid");
    }
}
