use std::collections::HashMap;

use serde_json::{Map, Number, Value, json};

use crate::errors::ApplicationError;

use super::content_parts::{
    InputPart, MediaPart, MediaSource, parse_openai_chat_content,
    reject_media_for_text_only_content,
};
use super::shared::message_content_to_text;
use super::tool_calls::{
    OpenAiToolCall, extract_openai_tool_calls, message_tool_call_id,
    validate_openai_chat_tool_transcript,
};

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

    match payload.get("tools") {
        None | Some(Value::Null) => {}
        Some(Value::Array(tools)) if tools.is_empty() => {}
        Some(Value::Array(tools)) => {
            request.insert(
                "tools".to_string(),
                Value::Array(map_openai_tools_to_interactions(tools)?),
            );
        }
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "Gemini Interactions tools must be an array".to_string(),
            ));
        }
    }

    if let Some(json_schema) = payload.get("json_schema").filter(|value| !value.is_null()) {
        let json_schema = json_schema.as_object().ok_or_else(|| {
            ApplicationError::ValidationError(
                "Gemini Interactions json_schema must be an object".to_string(),
            )
        })?;
        let schema_value = json_schema
            .get("value")
            .filter(|value| !value.is_null())
            .ok_or_else(|| {
                ApplicationError::ValidationError(
                    "Gemini Interactions json_schema is missing value".to_string(),
                )
            })?;
        request.insert(
            "response_format".to_string(),
            json!({
                "type": "text",
                "mime_type": "application/json",
                "schema": schema_value,
            }),
        );
    }

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

fn map_openai_tools_to_interactions(tools: &[Value]) -> Result<Vec<Value>, ApplicationError> {
    tools
        .iter()
        .enumerate()
        .map(|(index, tool)| {
            let tool = tool.as_object().ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "Gemini Interactions tool {index} must be an object"
                ))
            })?;

            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Ok(Value::Object(tool.clone()));
            }

            let function = tool
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "Gemini Interactions function tool {index} is missing function"
                    ))
                })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "Gemini Interactions function tool {index} is missing name"
                    ))
                })?;

            let mut mapped = Map::new();
            mapped.insert("type".to_string(), Value::String("function".to_string()));
            mapped.insert("name".to_string(), Value::String(name.to_string()));
            if let Some(description) = function.get("description").filter(|value| !value.is_null())
            {
                mapped.insert("description".to_string(), description.clone());
            }
            if let Some(parameters) = function.get("parameters").filter(|value| !value.is_null()) {
                mapped.insert("parameters".to_string(), parameters.clone());
            }

            Ok(Value::Object(mapped))
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

    let entries = messages.as_array().ok_or_else(|| {
        ApplicationError::ValidationError(
            "Gemini Interactions messages must be a string or an array".to_string(),
        )
    })?;
    validate_openai_chat_tool_transcript(Some(messages), false)?;

    let mut steps = Vec::new();
    let mut system_parts = Vec::new();
    let mut function_name_by_call_id = HashMap::<String, String>::new();

    for (index, entry) in entries.iter().enumerate() {
        let message = entry.as_object().ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Gemini Interactions message {index} must be an object"
            ))
        })?;

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|role| !role.is_empty())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "Gemini Interactions message {index} is missing role"
                ))
            })?;

        match role.as_str() {
            "system" | "developer" => {
                reject_media_for_text_only_content(
                    "Gemini Interactions system instruction",
                    message.get("content"),
                )?;
                let text = message_content_to_text(message.get("content"));
                if !text.trim().is_empty() {
                    system_parts.push(text);
                }
            }
            "assistant" => {
                let tool_calls = extract_openai_tool_calls(message.get("tool_calls"));
                for tool_call in &tool_calls {
                    function_name_by_call_id.insert(tool_call.id.clone(), tool_call.name.clone());
                }

                if let Some(native_steps) = message_native_steps(message)? {
                    let duplicate_split_turn = if tool_calls.is_empty() || index == 0 {
                        false
                    } else {
                        let previous = entries[index - 1].as_object().filter(|previous| {
                            previous
                                .get("role")
                                .and_then(Value::as_str)
                                .is_some_and(|role| role.eq_ignore_ascii_case("assistant"))
                                && extract_openai_tool_calls(previous.get("tool_calls")).is_empty()
                        });

                        match previous.map(message_native_steps).transpose()? {
                            Some(Some(previous_steps)) if previous_steps == native_steps => true,
                            Some(Some(_)) => {
                                return Err(ApplicationError::ValidationError(
                                    "Gemini Interactions split assistant turn has mismatched native steps"
                                        .to_string(),
                                ));
                            }
                            None | Some(None) => false,
                        }
                    };

                    if !duplicate_split_turn {
                        steps.extend_from_slice(native_steps);
                    }
                    continue;
                }

                let text = message_content_to_text(message.get("content"));
                if !text.trim().is_empty() {
                    steps.push(json!({
                        "type": "model_output",
                        "content": [{ "type": "text", "text": text }],
                    }));
                }

                steps.extend(tool_calls.iter().map(build_function_call_step));
            }
            "tool" | "function" => {
                let call_id = message_tool_call_id(message).ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "Tool message is missing tool_call_id required for Gemini Interactions function_result".to_string(),
                    )
                })?;
                let name = function_name_by_call_id.remove(&call_id).ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "Gemini Interactions function_result references call_id without preceding function_call: {call_id}"
                    ))
                })?;
                let result = convert_openai_content_to_interactions_blocks(message.get("content"))?;

                steps.push(json!({
                    "type": "function_result",
                    "name": name,
                    "call_id": call_id,
                    "result": result,
                }));
            }
            "user" => {
                let content =
                    convert_openai_content_to_interactions_blocks(message.get("content"))?;
                steps.push(json!({
                    "type": "user_input",
                    "content": content,
                }));
            }
            other => {
                return Err(ApplicationError::ValidationError(format!(
                    "Gemini Interactions message role is unsupported: {other}"
                )));
            }
        }
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

    Ok((Value::Array(steps), system_instruction))
}

fn message_native_steps(
    message: &Map<String, Value>,
) -> Result<Option<&[Value]>, ApplicationError> {
    let Some(native) = message.get("native") else {
        return Ok(None);
    };
    let native = native.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "Gemini Interactions message native state must be an object".to_string(),
        )
    })?;
    let Some(interactions) = native.get("gemini_interactions") else {
        return Ok(None);
    };
    let interactions = interactions.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "Gemini Interactions native state must be an object".to_string(),
        )
    })?;

    let steps = interactions
        .get("steps")
        .and_then(Value::as_array)
        .filter(|steps| !steps.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "Gemini Interactions native state is missing non-empty steps".to_string(),
            )
        })?;

    Ok(Some(steps))
}

fn build_function_call_step(tool_call: &OpenAiToolCall) -> Value {
    let mut step = json!({
        "type": "function_call",
        "id": tool_call.id.clone(),
        "name": tool_call.name.clone(),
        "arguments": tool_call.arguments.clone(),
    });

    if let Some(signature) = tool_call.signature.as_deref()
        && let Some(object) = step.as_object_mut()
    {
        object.insert(
            "signature".to_string(),
            Value::String(signature.to_string()),
        );
    }

    step
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
    fn gemini_interactions_builds_canonical_steps() {
        let payload = json!({
            "model": "gemini-3-flash-preview",
            "messages": [
                { "role": "system", "content": "System rule" },
                { "role": "developer", "content": "Developer rule" },
                { "role": "user", "content": "What is the weather in Paris?" },
                {
                    "role": "assistant",
                    "content": "Checking",
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
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "name": "wrong_projection_name",
                    "content": "Sunny"
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Gets the weather",
                        "parameters": { "type": "object" }
                    }
                },
                {
                    "type": "google_search",
                    "filters": { "safe_search": true }
                }
            ],
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/interactions");

        let upstream = upstream.as_object().expect("upstream must be object");
        assert_eq!(
            upstream["system_instruction"],
            "System rule\n\nDeveloper rule"
        );
        assert_eq!(upstream["tools"][1]["type"], "google_search");

        let input = upstream
            .get("input")
            .and_then(|v| v.as_array())
            .expect("input must be array");
        assert_eq!(input.len(), 4);
        assert_eq!(
            input[0],
            json!({
                "type": "user_input",
                "content": [{
                    "type": "text",
                    "text": "What is the weather in Paris?"
                }]
            })
        );
        assert_eq!(
            input[1],
            json!({
                "type": "model_output",
                "content": [{ "type": "text", "text": "Checking" }]
            })
        );
        assert_eq!(
            input[2],
            json!({
                "type": "function_call",
                "id": "call_1",
                "name": "get_weather",
                "arguments": { "location": "Paris" },
                "signature": "sig_1"
            })
        );
        assert_eq!(
            input[3],
            json!({
                "type": "function_result",
                "name": "get_weather",
                "call_id": "call_1",
                "result": [{ "type": "text", "text": "Sunny" }]
            })
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
            upstream.pointer("/input/0/type"),
            Some(&json!("user_input"))
        );
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
    fn gemini_interactions_coalesces_exact_split_native_turn() {
        let native_steps = json!([{
            "type": "function_call",
            "id": "call_1",
            "name": "provider_tool_name",
            "arguments": {},
            "signature": "opaque"
        }]);
        let mut messages = json!([
            {
                "role": "assistant",
                "content": "Visible tool call",
                "native": { "gemini_interactions": { "steps": native_steps.clone() } }
            },
            {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": { "name": "provider_tool_name", "arguments": "{}" }
                }],
                "native": { "gemini_interactions": { "steps": native_steps.clone() } }
            },
            {
                "role": "tool",
                "tool_call_id": "call_1",
                "name": "different_projection_name",
                "content": [{ "type": "text", "text": "Done" }]
            }
        ]);
        let upstream = build_with_messages(messages.clone());

        assert_eq!(
            upstream.get("input"),
            Some(&json!([
                {
                    "type": "function_call",
                    "id": "call_1",
                    "name": "provider_tool_name",
                    "arguments": {},
                    "signature": "opaque"
                },
                {
                    "type": "function_result",
                    "name": "provider_tool_name",
                    "call_id": "call_1",
                    "result": [{ "type": "text", "text": "Done" }]
                }
            ]))
        );

        let mismatched = messages
            .pointer_mut("/1/native/gemini_interactions/steps/0/name")
            .expect("native name must exist");
        *mismatched = json!("different");
        let payload = json!({
            "model": "gemini-3-flash-preview",
            "messages": messages,
        })
        .as_object()
        .cloned()
        .expect("payload must be object");
        let error = build(payload).expect_err("mismatched split native steps must fail");
        assert!(error.to_string().contains("mismatched native steps"));
    }

    #[test]
    fn gemini_interactions_wraps_json_schema_response_format() {
        let payload = json!({
            "model": "gemini-3-flash-preview",
            "messages": "Return JSON",
            "json_schema": {
                "value": {
                    "type": "object",
                    "properties": { "answer": { "type": "string" } },
                    "required": ["answer"]
                }
            }
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        assert_eq!(upstream.get("input"), Some(&json!("Return JSON")));
        assert_eq!(
            upstream.get("response_format"),
            Some(&json!({
                "type": "text",
                "mime_type": "application/json",
                "schema": {
                    "type": "object",
                    "properties": { "answer": { "type": "string" } },
                    "required": ["answer"]
                }
            }))
        );

        let malformed = json!({
            "model": "gemini-3-flash-preview",
            "messages": [{ "role": "user", "content": "Return JSON" }],
            "json_schema": {}
        })
        .as_object()
        .cloned()
        .expect("payload must be object");
        assert!(build(malformed).is_err());
    }
}
