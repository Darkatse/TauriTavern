use std::collections::HashMap;

use serde_json::{Map, Number, Value, json};

use crate::errors::ApplicationError;

use super::content_parts::{
    InputPart, MediaPart, MediaSource, parse_openai_chat_content,
    reject_media_for_text_only_content,
};
use super::shared::message_content_to_text;
use super::tool_calls::{
    OpenAiToolCall, extract_openai_tool_calls, fallback_tool_name, message_tool_call_id,
    message_tool_name, message_tool_result_text, normalize_tool_result_payload,
};

const CUSTOM_API_FORMAT: &str = "custom_api_format";

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let request = build_gemini_interactions_payload(&payload)?;

    Ok(("/interactions".to_string(), Value::Object(request)))
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
            ApplicationError::ValidationError(
                "Gemini Interactions request is missing model".to_string(),
            )
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

    if let Some(tools) = payload.get("tools").and_then(Value::as_array)
        && !tools.is_empty()
    {
        request.insert(
            "tools".to_string(),
            Value::Array(map_openai_tools_to_interactions(tools)),
        );
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
            let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or_default();

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
            reject_media_for_text_only_content(
                "Gemini Interactions system instruction",
                message.get("content"),
            )?;
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

        let content_blocks = convert_openai_content_to_interactions_blocks(message.get("content"))?;
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

    if let Some(signature) = signature
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
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

    if let Some(signature) = tool_call.signature.as_deref()
        && let Some(object) = output.as_object_mut()
    {
        object.insert(
            "signature".to_string(),
            Value::String(signature.to_string()),
        );
    }

    output
}

fn convert_openai_content_to_interactions_blocks(
    content: Option<&Value>,
) -> Result<Vec<Value>, ApplicationError> {
    let parts = parse_openai_chat_content(content)?;
    let mut blocks = Vec::with_capacity(parts.len());

    for part in &parts {
        if let Some(block) = render_interactions_block(part)? {
            blocks.push(block);
        }
    }

    Ok(blocks)
}

fn render_interactions_block(part: &InputPart) -> Result<Option<Value>, ApplicationError> {
    match part {
        InputPart::Text(text) => {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(json!({ "type": "text", "text": text })))
            }
        }
        InputPart::Image(media) => render_media_block("image", media).map(Some),
        InputPart::Audio(media) => render_media_block("audio", media).map(Some),
        InputPart::Video(media) => render_media_block("video", media).map(Some),
        InputPart::Unknown { ty, value } => render_unknown_block(ty.as_deref(), value).map(Some),
    }
}

fn render_media_block(kind: &str, media: &MediaPart) -> Result<Value, ApplicationError> {
    match &media.source {
        MediaSource::DataUrl { mime_type, data } => Ok(json!({
            "type": kind,
            "mime_type": mime_type,
            "data": data,
        })),
        MediaSource::Url(url) => Ok(json!({
            "type": kind,
            "uri": url,
        })),
        MediaSource::FileId(_) => Err(ApplicationError::ValidationError(
            "Gemini Interactions translator does not support provider file_id media references"
                .to_string(),
        )),
    }
}

fn render_unknown_block(ty: Option<&str>, value: &Value) -> Result<Value, ApplicationError> {
    if let Some(ty @ ("input_audio" | "input_video" | "input_file" | "file")) = ty {
        return Err(ApplicationError::ValidationError(format!(
            "Gemini Interactions content part type is unsupported: {ty}"
        )));
    }

    if value.is_object() {
        return Ok(value.clone());
    }

    Err(ApplicationError::ValidationError(
        "Gemini Interactions content part is unsupported".to_string(),
    ))
}

fn blocks_to_interactions_content_value(blocks: Vec<Value>) -> Value {
    if blocks.len() == 1
        && let Some(block_object) = blocks[0].as_object()
        && block_object.get("type").and_then(Value::as_str) == Some("text")
        && let Some(text) = block_object.get("text").and_then(Value::as_str)
    {
        return Value::String(text.to_string());
    }

    Value::Array(blocks)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use serde_json::json;

    use super::build;

    fn build_with_messages(messages: Value) -> Value {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "gemini_interactions",
            "model": "gemini-3-flash-preview",
            "messages": messages,
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/interactions");
        upstream
    }

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
        assert_eq!(
            upstream.get("model").and_then(|v| v.as_str()),
            Some("gemini-3-flash-preview")
        );
        assert!(upstream.get("tools").is_some());

        let input = upstream
            .get("input")
            .and_then(|v| v.as_array())
            .expect("input must be array");
        assert_eq!(input.len(), 3);

        assert_eq!(input[0].get("role").and_then(|v| v.as_str()), Some("user"));
        assert_eq!(input[1].get("role").and_then(|v| v.as_str()), Some("model"));

        let model_outputs = input[1]
            .get("content")
            .and_then(|v| v.as_array())
            .expect("model content must be array");
        assert_eq!(
            model_outputs[0].get("type").and_then(|v| v.as_str()),
            Some("function_call")
        );
        assert_eq!(
            model_outputs[0].get("id").and_then(|v| v.as_str()),
            Some("call_1")
        );

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

    #[test]
    fn gemini_interactions_builds_multimodal_user_blocks() {
        let upstream = build_with_messages(json!([{
            "role": "user",
            "content": [
                { "type": "text", "text": "describe" },
                {
                    "type": "image_url",
                    "image_url": {
                        "url": "data:image/png;base64,AAAA",
                        "detail": "high"
                    }
                },
                {
                    "type": "audio_url",
                    "audio_url": { "url": "data:audio/wav;base64,BBBB" }
                },
                {
                    "type": "video_url",
                    "video_url": {
                        "url": "https://example.test/video.mp4",
                        "detail": "low"
                    }
                }
            ]
        }]));

        assert_eq!(
            upstream.pointer("/input/0/content"),
            Some(&json!([
                { "type": "text", "text": "describe" },
                { "type": "image", "mime_type": "image/png", "data": "AAAA" },
                { "type": "audio", "mime_type": "audio/wav", "data": "BBBB" },
                { "type": "video", "uri": "https://example.test/video.mp4" }
            ]))
        );
    }

    #[test]
    fn gemini_interactions_preserves_native_input_blocks() {
        let native_image = json!({
            "type": "image",
            "uri": "https://example.test/cat.png",
            "mime_type": "image/png"
        });
        let custom_block = json!({
            "type": "custom_block",
            "payload": { "x": true }
        });
        let upstream = build_with_messages(json!([{
            "role": "user",
            "content": [native_image.clone(), custom_block.clone()]
        }]));

        assert_eq!(
            upstream.pointer("/input/0/content"),
            Some(&json!([native_image, custom_block]))
        );
    }

    #[test]
    fn gemini_interactions_rejects_malformed_media_parts() {
        for (content, expected) in [
            (
                json!([{ "type": "image_url", "image_url": { "detail": "high" } }]),
                "missing url",
            ),
            (
                json!([{
                    "type": "image_url",
                    "image_url": { "url": "data:image/png;base64," }
                }]),
                "invalid data URL",
            ),
        ] {
            let payload = json!({
                "chat_completion_source": "custom",
                "custom_api_format": "gemini_interactions",
                "model": "gemini-3-flash-preview",
                "messages": [{ "role": "user", "content": content }],
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let error = build(payload).expect_err("malformed media should fail");
            assert!(error.to_string().contains(expected));
        }
    }

    #[test]
    fn gemini_interactions_rejects_foreign_file_references() {
        for (content, expected) in [
            (
                json!([{ "type": "input_image", "file_id": "file_123" }]),
                "file_id",
            ),
            (
                json!([{ "type": "input_file", "file_id": "file_123" }]),
                "input_file",
            ),
        ] {
            let payload = json!({
                "chat_completion_source": "custom",
                "custom_api_format": "gemini_interactions",
                "model": "gemini-3-flash-preview",
                "messages": [{ "role": "user", "content": content }],
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let error = build(payload).expect_err("foreign file references should fail");
            assert!(error.to_string().contains(expected));
        }
    }

    #[test]
    fn gemini_interactions_rejects_system_media() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "gemini_interactions",
            "model": "gemini-3-flash-preview",
            "messages": [{
                "role": "system",
                "content": [
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                ]
            }],
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("system media should fail");
        assert!(
            error
                .to_string()
                .contains("system instruction cannot preserve image input")
        );
    }

    #[test]
    fn gemini_interactions_replays_native_outputs_before_synthetic_content() {
        let native_outputs = json!([
            { "type": "thought", "signature": "sig_native" },
            { "type": "text", "text": "Native text" }
        ]);
        let upstream = build_with_messages(json!([{
            "role": "assistant",
            "content": "Synthetic text",
            "signature": "sig_synthetic",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": "{}"
                }
            }],
            "native": {
                "gemini_interactions": {
                    "outputs": native_outputs.clone()
                }
            }
        }]));

        assert_eq!(upstream.pointer("/input/0/content"), Some(&native_outputs));
    }
}
