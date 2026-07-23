use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

use super::super::model_capabilities::{
    GeminiThinkingControl, RequestedReasoningEffort, is_gemini_thinking_config_model,
    map_gemini_thinking_control, parse_known_reasoning_effort,
};
use super::content_parts::{
    InputPart, MediaPart, MediaSource, parse_openai_chat_content,
    reject_media_for_text_only_content,
};
use super::shared::message_content_to_text;
use super::tool_calls::{
    OpenAiToolCall, extract_openai_tool_calls, fallback_tool_name, message_tool_call_id,
    message_tool_name, message_tool_result_text, normalize_tool_result_payload,
};

const GOOGLE_IMAGE_GENERATION_MODELS: &[&str] = &[
    "gemini-2.0-flash-exp",
    "gemini-2.0-flash-exp-image-generation",
    "gemini-2.0-flash-preview-image-generation",
    "gemini-2.5-flash-image-preview",
    "gemini-2.5-flash-image",
    "gemini-3-pro-image-preview",
];

const GOOGLE_NO_SEARCH_MODELS: &[&str] = &[
    "gemini-2.0-flash-lite",
    "gemini-2.0-flash-lite-001",
    "gemini-2.0-flash-lite-preview-02-05",
    "gemini-robotics-er-1.5-preview",
];

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    build_google_payload_with_mode(payload, false)
}

pub(super) fn build_vertexai(
    payload: Map<String, Value>,
) -> Result<(String, Value), ApplicationError> {
    build_google_payload_with_mode(payload, true)
}

fn build_google_payload_with_mode(
    payload: Map<String, Value>,
    use_vertex_ai: bool,
) -> Result<(String, Value), ApplicationError> {
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
        Value::Object(build_google_payload(&payload, use_vertex_ai)?),
    ))
}

fn build_google_payload(
    payload: &Map<String, Value>,
    use_vertex_ai: bool,
) -> Result<Map<String, Value>, ApplicationError> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Gemini request is missing model".to_string())
        })?;

    let enable_web_search = payload
        .get("enable_web_search")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let request_images = payload
        .get("request_images")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let aspect_ratio = payload
        .get("request_image_aspect_ratio")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let image_size = payload
        .get("request_image_resolution")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let is_gemma = model.contains("gemma");
    let is_learnlm = model.contains("learnlm");

    let enable_image_modality = request_images && GOOGLE_IMAGE_GENERATION_MODELS.contains(&model);

    let use_system_prompt = payload
        .get("use_sysprompt")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !enable_image_modality
        && !is_gemma;

    let (contents, system_prompt) = convert_messages(
        payload.get("messages"),
        model,
        use_system_prompt,
        use_vertex_ai,
    )?;

    let mut generation_config = Map::new();
    let has_fixed_sampling_parameters =
        matches!(model, "gemini-3.5-flash-lite" | "gemini-3.6-flash");

    if let Some(value) = payload.get("max_tokens").filter(|value| !value.is_null()) {
        generation_config.insert("maxOutputTokens".to_string(), value.clone());
    }

    for (source_key, target_key) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("top_k", "topK"),
        ("seed", "seed"),
    ] {
        if has_fixed_sampling_parameters && source_key != "seed" {
            continue;
        }

        if source_key == "top_k"
            && payload
                .get(source_key)
                .and_then(Value::as_i64)
                .is_some_and(|value| value == 0)
        {
            continue;
        }

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

    let response_mime_type = payload
        .get("responseMimeType")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Value::String(value.to_string()))
        .or_else(|| {
            payload
                .get("json_schema")
                .and_then(Value::as_object)
                .and_then(|schema| schema.get("value"))
                .filter(|value| !value.is_null())
                .map(|_| Value::String("application/json".to_string()))
        });

    let response_schema = payload
        .get("responseSchema")
        .cloned()
        .filter(|value| !value.is_null())
        .or_else(|| {
            payload
                .get("json_schema")
                .and_then(Value::as_object)
                .and_then(|schema| schema.get("value"))
                .cloned()
                .filter(|value| !value.is_null())
        });

    if let Some(response_mime_type) = response_mime_type {
        generation_config.insert("responseMimeType".to_string(), response_mime_type);
    }

    if let Some(response_schema) = response_schema {
        generation_config.insert("responseSchema".to_string(), response_schema);
    }

    if enable_image_modality {
        generation_config.insert("responseModalities".to_string(), json!(["text", "image"]));

        let enable_image_config = aspect_ratio.is_some() || image_size.is_some();
        if enable_image_config {
            let mut image_config = Map::new();

            if let Some(image_size) = image_size.filter(|_| is_google_image_size_model(model)) {
                image_config.insert(
                    "imageSize".to_string(),
                    Value::String(image_size.to_string()),
                );
            }

            if let Some(aspect_ratio) = aspect_ratio {
                image_config.insert(
                    "aspectRatio".to_string(),
                    Value::String(aspect_ratio.to_string()),
                );
            }

            if !image_config.is_empty() {
                generation_config.insert("imageConfig".to_string(), Value::Object(image_config));
            }
        }
    }

    inject_google_thinking_config(payload, model, use_vertex_ai, &mut generation_config)?;

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

    request.insert(
        "safetySettings".to_string(),
        Value::Array(google_safety_settings(use_vertex_ai)),
    );

    if use_system_prompt && !system_prompt.is_empty() {
        request.insert(
            "systemInstruction".to_string(),
            json!({
                "parts": [{ "text": system_prompt }],
            }),
        );
    }

    let mut tools = Vec::<Value>::new();

    if !enable_image_modality && !is_gemma {
        if let Some(raw_tools) = payload.get("tools") {
            let (function_declarations, custom_tools) = split_openai_tools(raw_tools);

            if !function_declarations.is_empty() {
                tools.push(json!({ "function_declarations": function_declarations }));
            } else if !custom_tools.is_empty() {
                tools.extend(custom_tools);
            }
        }

        if enable_web_search
            && !is_learnlm
            && !GOOGLE_NO_SEARCH_MODELS.contains(&model)
            && !tools
                .iter()
                .any(|tool| tool.get("function_declarations").is_some())
        {
            tools.push(json!({ "google_search": {} }));
        }
    }

    if !tools.is_empty() {
        request.insert("tools".to_string(), Value::Array(tools));

        if let Some(tool_choice) = payload
            .get("tool_choice")
            .and_then(map_tool_choice_to_makersuite)
            .filter(|_| request_has_function_declarations(&request))
        {
            request.insert(
                "toolConfig".to_string(),
                json!({ "functionCallingConfig": tool_choice }),
            );
        }
    }

    Ok(request)
}

fn convert_messages(
    messages: Option<&Value>,
    model: &str,
    use_system_prompt: bool,
    use_vertex_ai: bool,
) -> Result<(Vec<Value>, String), ApplicationError> {
    let mut contents = Vec::new();
    let mut system_parts = Vec::new();
    let mut tool_name_by_id: HashMap<String, String> = HashMap::new();

    let Some(messages) = messages else {
        return Ok((contents, String::new()));
    };

    if let Some(prompt) = messages.as_str() {
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": prompt }],
        }));
        return Ok((contents, String::new()));
    }

    let Some(entries) = messages.as_array() else {
        return Ok((contents, String::new()));
    };

    let model_lower = model.trim().to_ascii_lowercase();
    let supports_signatures =
        model_lower.contains("gemini-3") || model_lower.contains("gemini-2.5");
    let is_gemini3 = model_lower.contains("gemini-3");
    let supports_function_call_ids = is_gemini3 && !use_vertex_ai;
    let is_image_model = model_lower.contains("-image");
    let skip_signature_magic = "skip_thought_signature_validator";

    let mut start_index = 0_usize;
    if use_system_prompt && entries.len() > 1 {
        while start_index < entries.len().saturating_sub(1) {
            let Some(message) = entries[start_index].as_object() else {
                break;
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

            reject_media_for_text_only_content(
                "Gemini system instruction",
                message.get("content"),
            )?;
            let content_text = message_content_to_text(message.get("content"));
            if !content_text.is_empty() {
                system_parts.push(content_text);
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
        let mut merge_with_previous = matches!(role.as_str(), "tool" | "function");

        let mut parts = if matches!(role.as_str(), "tool" | "function") {
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
            let response_id = if supports_function_call_ids {
                tool_call_id.as_deref()
            } else {
                None
            };
            vec![build_tool_response_part(&name, &content, response_id)]
        } else {
            let native_gemini_parts = if role == "assistant" {
                message_native_gemini_parts(message)
            } else {
                None
            };
            let mut parts = if let Some(native_parts) = native_gemini_parts.clone() {
                native_parts
            } else {
                convert_message_content_to_parts(message.get("content"), is_gemini3)?
            };

            if role == "assistant" {
                let tool_calls = extract_openai_tool_calls(message.get("tool_calls"));
                if !tool_calls.is_empty() {
                    merge_with_previous = true;
                    for tool_call in &tool_calls {
                        tool_name_by_id.insert(tool_call.id.clone(), tool_call.name.clone());
                    }
                    if native_gemini_parts.is_none() {
                        parts.extend(convert_openai_tool_calls_to_parts(
                            &tool_calls,
                            supports_function_call_ids,
                        ));
                    }
                }
            }

            parts
        };

        if parts.is_empty() {
            parts.push(json!({ "text": "" }));
        }

        let target_role = if role == "assistant" { "model" } else { "user" };

        if supports_signatures {
            let text_signature = message
                .get("signature")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());

            for part in &mut parts {
                let Some(part_object) = part.as_object_mut() else {
                    continue;
                };

                let is_text_part = part_object.get("text").and_then(Value::as_str).is_some();

                if let Some(text_signature) = text_signature
                    && is_text_part
                {
                    part_object.insert(
                        "thoughtSignature".to_string(),
                        Value::String(text_signature.to_string()),
                    );
                    continue;
                }

                if is_gemini3 {
                    if part_object.get("functionCall").is_some()
                        && !part_object.contains_key("thoughtSignature")
                    {
                        part_object.insert(
                            "thoughtSignature".to_string(),
                            Value::String(skip_signature_magic.to_string()),
                        );
                    }

                    if is_image_model
                        && target_role == "model"
                        && (is_text_part || part_object.get("inlineData").is_some())
                    {
                        part_object.insert(
                            "thoughtSignature".to_string(),
                            Value::String(skip_signature_magic.to_string()),
                        );
                    }
                }
            }
        }

        if merge_with_previous
            && contents
                .last()
                .and_then(|content| content.get("role"))
                .and_then(Value::as_str)
                == Some(target_role)
        {
            contents
                .last_mut()
                .and_then(|content| content.get_mut("parts"))
                .and_then(Value::as_array_mut)
                .expect("Google content parts must be an array")
                .extend(parts);
        } else {
            contents.push(json!({
                "role": target_role,
                "parts": parts,
            }));
        }
    }

    Ok((contents, system_parts.join("\n\n")))
}

fn convert_message_content_to_parts(
    content: Option<&Value>,
    is_gemini3: bool,
) -> Result<Vec<Value>, ApplicationError> {
    let parts = parse_openai_chat_content(content)?;
    let mut rendered = Vec::with_capacity(parts.len());

    for part in &parts {
        if let Some(part) = render_google_content_part(part, is_gemini3)? {
            rendered.push(part);
        }
    }

    Ok(rendered)
}

fn render_google_content_part(
    part: &InputPart,
    is_gemini3: bool,
) -> Result<Option<Value>, ApplicationError> {
    match part {
        InputPart::Text(text) => {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(json!({ "text": text })))
            }
        }
        InputPart::Image(media) => render_google_media_part("image", media, is_gemini3).map(Some),
        InputPart::Audio(media) => render_google_media_part("audio", media, false).map(Some),
        InputPart::Video(media) => render_google_media_part("video", media, is_gemini3).map(Some),
        InputPart::Unknown { ty, value } => {
            render_google_unknown_content_part(ty.as_deref(), value).map(Some)
        }
    }
}

fn render_google_media_part(
    kind: &str,
    media: &MediaPart,
    use_media_resolution: bool,
) -> Result<Value, ApplicationError> {
    match &media.source {
        MediaSource::DataUrl { mime_type, data } => {
            let mut part = json!({
                "inlineData": {
                    "mimeType": mime_type,
                    "data": data,
                }
            });

            if use_media_resolution
                && let Some(level) = media.detail.as_deref().and_then(gemini_media_resolution)
                && let Some(part_object) = part.as_object_mut()
            {
                part_object.insert("mediaResolution".to_string(), json!({ "level": level }));
            }

            Ok(part)
        }
        MediaSource::Url(_) => Err(ApplicationError::ValidationError(format!(
            "Gemini generateContent translator does not support remote {kind} URLs yet; send a data URL or use Gemini Interactions for URI media input."
        ))),
        MediaSource::FileId(_) => Err(ApplicationError::ValidationError(
            "Gemini generateContent translator does not support provider file_id media references"
                .to_string(),
        )),
    }
}

fn render_google_unknown_content_part(
    ty: Option<&str>,
    value: &Value,
) -> Result<Value, ApplicationError> {
    if let Some(object) = value.as_object() {
        if ty.is_none()
            && let Some(native_part) = render_google_native_content_part(object)?
        {
            return Ok(native_part);
        }

        if let Some(text) = object
            .get("text")
            .or_else(|| object.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(json!({ "text": text }));
        }
    }

    Err(ApplicationError::ValidationError(match ty {
        Some(ty) => format!("Gemini generateContent content part type is unsupported: {ty}"),
        None => "Gemini generateContent content part is unsupported".to_string(),
    }))
}

fn render_google_native_content_part(
    object: &Map<String, Value>,
) -> Result<Option<Value>, ApplicationError> {
    let mut part = object.clone();

    if normalize_google_part_object(
        &mut part,
        "inlineData",
        "inline_data",
        &[("mime_type", "mimeType")],
    )? {
        return Ok(Some(Value::Object(part)));
    }

    if normalize_google_part_object(
        &mut part,
        "fileData",
        "file_data",
        &[("mime_type", "mimeType"), ("file_uri", "fileUri")],
    )? {
        return Ok(Some(Value::Object(part)));
    }

    if normalize_google_part_object(&mut part, "functionCall", "function_call", &[])? {
        return Ok(Some(Value::Object(part)));
    }

    if normalize_google_part_object(&mut part, "functionResponse", "function_response", &[])? {
        return Ok(Some(Value::Object(part)));
    }

    Ok(None)
}

fn normalize_google_part_object(
    part: &mut Map<String, Value>,
    canonical: &str,
    legacy: &str,
    field_aliases: &[(&str, &str)],
) -> Result<bool, ApplicationError> {
    let key = if part.contains_key(canonical) {
        canonical
    } else if part.contains_key(legacy) {
        legacy
    } else {
        return Ok(false);
    };

    let value = part
        .remove(key)
        .expect("Google native content part key must exist");
    let Value::Object(mut object) = value else {
        return Err(ApplicationError::ValidationError(format!(
            "Gemini native content part {key} must be an object"
        )));
    };

    for (legacy, canonical) in field_aliases {
        normalize_google_part_field(&mut object, legacy, canonical);
    }
    part.remove(legacy);
    part.insert(canonical.to_string(), Value::Object(object));
    Ok(true)
}

fn normalize_google_part_field(object: &mut Map<String, Value>, legacy: &str, canonical: &str) {
    if let Some(value) = object.remove(legacy)
        && !object.contains_key(canonical)
    {
        object.insert(canonical.to_string(), value);
    }
}

fn message_native_gemini_parts(message: &Map<String, Value>) -> Option<Vec<Value>> {
    message
        .get("native")?
        .get("gemini")?
        .get("content")?
        .get("parts")?
        .as_array()
        .cloned()
}

fn convert_openai_tool_calls_to_parts(
    tool_calls: &[OpenAiToolCall],
    supports_function_call_ids: bool,
) -> Vec<Value> {
    tool_calls
        .iter()
        .map(|tool_call| {
            let mut function_call = json!({
                "name": tool_call.name,
                "args": tool_call.arguments,
            });
            if supports_function_call_ids {
                function_call["id"] = Value::String(tool_call.id.clone());
            }
            let mut part = json!({ "functionCall": function_call });

            if let Some(signature) = tool_call.signature.as_ref()
                && let Some(part_object) = part.as_object_mut()
            {
                part_object.insert(
                    "thoughtSignature".to_string(),
                    Value::String(signature.clone()),
                );
            }

            part
        })
        .collect()
}

fn build_tool_response_part(name: &str, content: &str, id: Option<&str>) -> Value {
    let mut function_response = json!({
        "name": name,
        "response": normalize_tool_result_payload(content),
    });
    if let Some(id) = id {
        function_response["id"] = Value::String(id.to_string());
    }
    json!({ "functionResponse": function_response })
}

fn split_openai_tools(tools: &Value) -> (Vec<Value>, Vec<Value>) {
    let Some(entries) = tools.as_array() else {
        return (Vec::new(), Vec::new());
    };

    let mut function_declarations = Vec::<Value>::new();
    let mut custom_tools = Vec::<Value>::new();

    for entry in entries {
        let Some(tool) = entry.as_object() else {
            continue;
        };

        let tool_type = tool
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let Some(tool_type) = tool_type else {
            continue;
        };

        if tool_type == "function" {
            let Some(function) = tool.get("function").and_then(Value::as_object) else {
                continue;
            };

            let mut function = function.clone();

            if let Some(parameters) = function
                .get_mut("parameters")
                .and_then(Value::as_object_mut)
            {
                parameters.remove("$schema");

                if parameters
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_some_and(|properties| properties.is_empty())
                {
                    function.remove("parameters");
                }
            }

            function_declarations.push(Value::Object(function));
            continue;
        }

        let Some(custom_tool) = tool.get(tool_type) else {
            continue;
        };

        let mut custom_tool_object = Map::new();
        custom_tool_object.insert(tool_type.to_string(), custom_tool.clone());
        custom_tools.push(Value::Object(custom_tool_object));
    }

    (function_declarations, custom_tools)
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
    use_vertex_ai: bool,
    generation_config: &mut Map<String, Value>,
) -> Result<(), ApplicationError> {
    let reasoning_effort = match payload.get("reasoning_effort").and_then(Value::as_str) {
        Some(value) => parse_known_reasoning_effort(value, "Gemini")?,
        None => RequestedReasoningEffort::Auto,
    };

    if !is_gemini_thinking_config_model(model) {
        return Ok(());
    }

    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_output_tokens = generation_config
        .get("maxOutputTokens")
        .and_then(value_to_i64)
        .unwrap_or(0);

    let mut thinking_config = Map::new();
    let mut include_thoughts = include_reasoning;

    if let Some(control) = map_gemini_thinking_control(model, max_output_tokens, reasoning_effort)?
    {
        match control {
            GeminiThinkingControl::BudgetTokens(tokens) => {
                thinking_config.insert(
                    "thinkingBudget".to_string(),
                    Value::Number(serde_json::Number::from(tokens)),
                );

                if use_vertex_ai && tokens == 0 && include_thoughts {
                    include_thoughts = false;
                }
            }
            GeminiThinkingControl::Level(level) => {
                thinking_config.insert(
                    "thinkingLevel".to_string(),
                    Value::String(level.to_string()),
                );
            }
        }
    }

    thinking_config.insert("includeThoughts".to_string(), Value::Bool(include_thoughts));

    generation_config.insert("thinkingConfig".to_string(), Value::Object(thinking_config));
    Ok(())
}

fn is_google_image_size_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gemini-3")
}

fn request_has_function_declarations(request: &Map<String, Value>) -> bool {
    request
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool.get("function_declarations").is_some())
        })
}

fn google_safety_settings(use_vertex_ai: bool) -> Vec<Value> {
    let mut settings = vec![
        json!({ "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" }),
        json!({ "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" }),
        json!({ "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" }),
        json!({ "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" }),
        json!({ "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" }),
    ];

    if use_vertex_ai {
        settings.extend([
            json!({ "category": "HARM_CATEGORY_IMAGE_HATE", "threshold": "OFF" }),
            json!({ "category": "HARM_CATEGORY_IMAGE_DANGEROUS_CONTENT", "threshold": "OFF" }),
            json!({ "category": "HARM_CATEGORY_IMAGE_HARASSMENT", "threshold": "OFF" }),
            json!({ "category": "HARM_CATEGORY_IMAGE_SEXUALLY_EXPLICIT", "threshold": "OFF" }),
            json!({ "category": "HARM_CATEGORY_JAILBREAK", "threshold": "OFF" }),
        ]);
    }

    settings
}

fn gemini_media_resolution(detail: &str) -> Option<&'static str> {
    match detail.trim() {
        "low" => Some("media_resolution_low"),
        "high" => Some("media_resolution_high"),
        _ => None,
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{build, build_vertexai};

    fn build_with_messages(model: &str, messages: Value) -> Value {
        let payload = json!({
            "model": model,
            "messages": messages
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        build(payload).expect("build should succeed").1
    }

    #[test]
    fn makersuite_fixed_sampling_models_only_forward_seed() {
        let build_config = |model: &str| {
            let payload = json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "temperature": 0.7,
                "top_p": 0.9,
                "top_k": 40,
                "seed": 17
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            build(payload)
                .expect("build should succeed")
                .1
                .get("generationConfig")
                .and_then(Value::as_object)
                .cloned()
                .expect("generationConfig must be object")
        };

        for model in ["gemini-3.5-flash-lite", "gemini-3.6-flash"] {
            let config = build_config(model);
            for key in ["candidateCount", "temperature", "topP", "topK"] {
                assert!(config.get(key).is_none(), "{model} must omit {key}");
            }
            assert_eq!(config.get("seed").and_then(Value::as_i64), Some(17));
        }

        let config = build_config("gemini-2.5-flash");
        assert_eq!(config.get("temperature").and_then(Value::as_f64), Some(0.7));
        assert_eq!(config.get("topP").and_then(Value::as_f64), Some(0.9));
        assert_eq!(config.get("topK").and_then(Value::as_i64), Some(40));
    }

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
        assert!(
            thinking
                .get("includeThoughts")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
    }

    #[test]
    fn makersuite_25_flash_accepts_shared_minimal_alias() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 4000,
            "reasoning_effort": "minimal"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        assert_eq!(
            upstream
                .pointer("/generationConfig/thinkingConfig/thinkingBudget")
                .and_then(Value::as_i64),
            Some(0)
        );
    }

    #[test]
    fn makersuite_25_flash_lite_auto_omits_thinking_budget() {
        let payload = json!({
            "model": "gemini-2.5-flash-lite",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 4000,
            "reasoning_effort": "auto"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        assert!(
            upstream
                .pointer("/generationConfig/thinkingConfig/thinkingBudget")
                .is_none()
        );
        assert_eq!(
            upstream
                .pointer("/generationConfig/thinkingConfig/includeThoughts")
                .and_then(Value::as_bool),
            Some(false)
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
    fn makersuite_31_pro_sets_medium_thinking_level() {
        let payload = json!({
            "model": "gemini-3.1-pro-preview",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8000,
            "reasoning_effort": "medium"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        assert_eq!(
            upstream
                .pointer("/generationConfig/thinkingConfig/thinkingLevel")
                .and_then(Value::as_str),
            Some("medium")
        );
        assert!(
            upstream
                .pointer("/generationConfig/thinkingConfig/thinkingBudget")
                .is_none()
        );
    }

    #[test]
    fn makersuite_31_flash_lite_uses_level_not_budget() {
        let payload = json!({
            "model": "gemini-3.1-flash-lite-preview",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8000,
            "reasoning_effort": "medium"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        assert_eq!(
            upstream
                .pointer("/generationConfig/thinkingConfig/thinkingLevel")
                .and_then(Value::as_str),
            Some("medium")
        );
        assert!(
            upstream
                .pointer("/generationConfig/thinkingConfig/thinkingBudget")
                .is_none()
        );
    }

    #[test]
    fn makersuite_xhigh_behaves_like_max() {
        let payload = json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 8000,
            "reasoning_effort": "xhigh"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        assert_eq!(
            upstream
                .pointer("/generationConfig/thinkingConfig/thinkingBudget")
                .and_then(Value::as_i64),
            Some(8000)
        );
    }

    #[test]
    fn makersuite_rejects_unknown_reasoning_effort() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "turbo"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("unknown effort should fail locally");
        assert!(
            error
                .to_string()
                .contains("Unsupported Gemini reasoning_effort")
        );
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
    fn makersuite_3_image_model_sets_thinking_level() {
        let payload = json!({
            "model": "gemini-3-pro-image-preview",
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
        let thinking = config
            .get("thinkingConfig")
            .and_then(Value::as_object)
            .expect("thinkingConfig must be object");

        assert_eq!(
            thinking.get("thinkingLevel").and_then(Value::as_str),
            Some("high")
        );
        assert!(thinking.get("thinkingBudget").is_none());
        assert_eq!(
            thinking.get("includeThoughts").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn makersuite_builds_multimodal_inline_parts() {
        let upstream = build_with_messages(
            "gemini-3-pro",
            json!([{
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
                        "type": "video_url",
                        "video_url": {
                            "url": "data:video/mp4;base64,BBBB",
                            "detail": "low"
                        }
                    },
                    {
                        "type": "audio_url",
                        "audio_url": { "url": "data:audio/wav;base64,CCCC" }
                    }
                ]
            }]),
        );

        assert_eq!(
            upstream.pointer("/contents/0/parts"),
            Some(&json!([
                { "text": "describe" },
                {
                    "inlineData": {
                        "mimeType": "image/png",
                        "data": "AAAA"
                    },
                    "mediaResolution": { "level": "media_resolution_high" }
                },
                {
                    "inlineData": {
                        "mimeType": "video/mp4",
                        "data": "BBBB"
                    },
                    "mediaResolution": { "level": "media_resolution_low" }
                },
                {
                    "inlineData": {
                        "mimeType": "audio/wav",
                        "data": "CCCC"
                    }
                }
            ]))
        );
    }

    #[test]
    fn makersuite_omits_auto_media_resolution() {
        let upstream = build_with_messages(
            "gemini-3-pro",
            json!([{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": {
                        "url": "data:image/png;base64,AAAA",
                        "detail": "auto"
                    }
                }]
            }]),
        );

        assert!(
            upstream
                .pointer("/contents/0/parts/0/mediaResolution")
                .is_none()
        );
    }

    #[test]
    fn makersuite_preserves_native_google_parts() {
        let function_call = json!({
            "name": "lookup",
            "args": { "query": "weather" }
        });
        let function_response = json!({
            "name": "lookup",
            "response": { "content": "sunny" }
        });
        let upstream = build_with_messages(
            "gemini-2.5-flash",
            json!([{
                "role": "user",
                "content": [
                    {
                        "inlineData": { "mimeType": "image/png", "data": "AAAA" },
                        "mediaResolution": { "level": "media_resolution_low" }
                    },
                    {
                        "inline_data": { "mime_type": "image/jpeg", "data": "BBBB" },
                        "mediaResolution": { "level": "media_resolution_high" }
                    },
                    { "fileData": { "mimeType": "image/png", "fileUri": "gs://bucket/cat.png" } },
                    { "file_data": { "mime_type": "image/jpeg", "file_uri": "https://example.test/cat.jpg" } },
                    {
                        "functionCall": function_call.clone(),
                        "thoughtSignature": "sig_call"
                    },
                    { "function_call": function_call.clone() },
                    { "functionResponse": function_response.clone() },
                    { "function_response": function_response.clone() }
                ]
            }]),
        );

        assert_eq!(
            upstream.pointer("/contents/0/parts"),
            Some(&json!([
                {
                    "inlineData": { "mimeType": "image/png", "data": "AAAA" },
                    "mediaResolution": { "level": "media_resolution_low" }
                },
                {
                    "inlineData": { "mimeType": "image/jpeg", "data": "BBBB" },
                    "mediaResolution": { "level": "media_resolution_high" }
                },
                { "fileData": { "mimeType": "image/png", "fileUri": "gs://bucket/cat.png" } },
                { "fileData": { "mimeType": "image/jpeg", "fileUri": "https://example.test/cat.jpg" } },
                {
                    "functionCall": function_call.clone(),
                    "thoughtSignature": "sig_call"
                },
                { "functionCall": function_call },
                { "functionResponse": function_response.clone() },
                { "functionResponse": function_response }
            ]))
        );
    }

    #[test]
    fn makersuite_rejects_malformed_native_google_parts() {
        for (part, expected) in [
            (
                json!({ "inlineData": "bad", "text": "fallback" }),
                "inlineData",
            ),
            (
                json!({ "inline_data": "bad", "text": "fallback" }),
                "inline_data",
            ),
            (json!({ "fileData": "bad", "text": "fallback" }), "fileData"),
            (
                json!({ "functionCall": "bad", "text": "fallback" }),
                "functionCall",
            ),
            (
                json!({ "function_call": "bad", "text": "fallback" }),
                "function_call",
            ),
            (
                json!({ "functionResponse": "bad", "text": "fallback" }),
                "functionResponse",
            ),
            (
                json!({ "function_response": "bad", "text": "fallback" }),
                "function_response",
            ),
        ] {
            let payload = json!({
                "model": "gemini-2.5-flash",
                "messages": [{ "role": "user", "content": [part] }]
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let error = build(payload).expect_err("malformed native part should fail fast");
            assert!(
                error.to_string().contains(expected),
                "expected {expected:?}, got {error}"
            );
        }
    }

    #[test]
    fn makersuite_keeps_unknown_text_parts_as_text() {
        let upstream = build_with_messages(
            "gemini-2.5-flash",
            json!([{
                "role": "user",
                "content": [
                    { "type": "provider_text", "text": "hello" },
                    { "type": "provider_content", "content": " world " }
                ]
            }]),
        );

        assert_eq!(
            upstream.pointer("/contents/0/parts"),
            Some(&json!([
                { "text": "hello" },
                { "text": "world" }
            ]))
        );
    }

    #[test]
    fn makersuite_rejects_media_that_generate_content_cannot_preserve() {
        for (content, expected) in [
            (
                json!([{ "type": "image_url", "image_url": { "detail": "high" } }]),
                "missing url",
            ),
            (
                json!([{
                    "type": "audio_url",
                    "audio_url": { "url": "data:audio/wav;base64," }
                }]),
                "invalid data URL",
            ),
            (
                json!([{
                    "type": "image_url",
                    "image_url": { "url": "https://example.test/cat.png" }
                }]),
                "remote image URLs",
            ),
            (
                json!([{ "type": "input_image", "file_id": "file_123" }]),
                "file_id",
            ),
        ] {
            let payload = json!({
                "model": "gemini-2.5-flash",
                "messages": [{ "role": "user", "content": content }]
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let error = build(payload).expect_err("unsupported media should fail fast");
            assert!(
                error.to_string().contains(expected),
                "expected {expected:?}, got {error}"
            );
        }
    }

    #[test]
    fn makersuite_rejects_system_media_when_hoisted() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "use_sysprompt": true,
            "messages": [
                {
                    "role": "system",
                    "content": [{
                        "type": "image_url",
                        "image_url": { "url": "data:image/png;base64,AAAA" }
                    }]
                },
                { "role": "user", "content": "hello" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("system media should fail fast");
        assert!(
            error
                .to_string()
                .contains("Gemini system instruction cannot preserve image input")
        );
    }

    #[test]
    fn vertexai_uses_shared_multimodal_renderer() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "audio_url",
                    "audio_url": { "url": "data:audio/wav;base64,AAAA" }
                }]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build_vertexai(payload).expect("build should succeed");
        assert_eq!(
            upstream.pointer("/contents/0/parts/0/inlineData"),
            Some(&json!({ "mimeType": "audio/wav", "data": "AAAA" }))
        );
    }

    #[test]
    fn makersuite_tool_result_uses_previous_tool_call_name() {
        let payload = json!({
            "model": "gemini-3.6-flash",
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

        let (_, upstream) = build(payload.clone()).expect("build should succeed");
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
        assert_eq!(
            model_part.get("id").and_then(Value::as_str),
            Some("call_weather")
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
            user_part.get("id").and_then(Value::as_str),
            Some("call_weather")
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

        let mut legacy_payload = payload.clone();
        legacy_payload.insert(
            "model".to_string(),
            Value::String("gemini-2.5-flash".to_string()),
        );
        let (_, legacy) = build(legacy_payload).expect("legacy build should succeed");
        let (_, vertex) = build_vertexai(payload).expect("Vertex build should succeed");
        for body in [&legacy, &vertex] {
            assert!(
                body.pointer("/contents/0/parts/0/functionCall/id")
                    .is_none()
            );
            assert!(
                body.pointer("/contents/1/parts/0/functionResponse/id")
                    .is_none()
            );
        }
    }

    #[test]
    fn makersuite_merges_parallel_tool_turns() {
        let upstream = build_with_messages(
            "gemini-3.6-flash",
            json!([
                { "role": "assistant", "content": "Visible canonical text" },
                {
                    "role": "assistant",
                    "tool_calls": [
                        { "id": "call_weather", "function": { "name": "weather", "arguments": "{}" } },
                        { "id": "call_time", "function": { "name": "local_time", "arguments": "{}" } }
                    ]
                },
                { "role": "tool", "tool_call_id": "call_weather", "content": "sunny" },
                { "role": "tool", "tool_call_id": "call_time", "content": "12:00" }
            ]),
        );

        let contents = upstream
            .get("contents")
            .and_then(Value::as_array)
            .expect("contents must be array");
        assert_eq!(contents.len(), 2, "consecutive model and tool turns merge");

        let model_parts = contents[0]["parts"].as_array().expect("model parts");
        assert_eq!(model_parts[0]["text"], "Visible canonical text");
        assert_eq!(model_parts[1]["functionCall"]["id"], "call_weather");
        assert_eq!(model_parts[2]["functionCall"]["id"], "call_time");

        let tool_parts = contents[1]["parts"].as_array().expect("tool parts");
        assert_eq!(tool_parts.len(), 2, "parallel tool results share one turn");
        assert_eq!(tool_parts[0]["functionResponse"]["id"], "call_weather");
        assert_eq!(tool_parts[1]["functionResponse"]["id"], "call_time");
    }

    #[test]
    fn makersuite_tool_result_skips_user_content_part_renderer() {
        for role in ["tool", "function"] {
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
                        "role": role,
                        "tool_call_id": "call_weather",
                        "content": [
                            { "type": "text", "text": "{\"temperature\":20}" },
                            {
                                "type": "image_url",
                                "image_url": { "url": "https://example.test/tool-output.png" }
                            }
                        ]
                    }
                ]
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let (_, upstream) =
                build(payload).expect("tool result should not render as user media");
            assert_eq!(
                upstream.pointer("/contents/1/parts/0/functionResponse/response/temperature"),
                Some(&json!(20))
            );
        }
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

    #[test]
    fn makersuite_inlines_system_messages_when_sysprompt_disabled() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "use_sysprompt": false,
            "messages": [
                {"role": "system", "content": "SYS"},
                {"role": "user", "content": "hello"}
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        assert!(body.get("systemInstruction").is_none());

        let contents = body
            .get("contents")
            .and_then(Value::as_array)
            .expect("contents must be array");
        let first = contents
            .first()
            .and_then(Value::as_object)
            .expect("first content must be object");
        assert_eq!(first.get("role").and_then(Value::as_str), Some("user"));
        let first_text = first
            .get("parts")
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(Value::as_object)
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(first_text, "SYS");
    }

    #[test]
    fn makersuite_enable_web_search_adds_google_search_tool() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "enable_web_search": true,
            "messages": [{"role": "user", "content": "hello"}]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let tools = body
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools must be array");

        assert!(tools.iter().any(|tool| tool.get("google_search").is_some()));
    }

    #[test]
    fn makersuite_image_generation_sets_response_modalities_and_image_config() {
        let payload = json!({
            "model": "gemini-3-pro-image-preview",
            "request_images": true,
            "request_image_resolution": "image_size_1",
            "request_image_aspect_ratio": "16:9",
            "messages": [{"role": "user", "content": "hello"}]
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

        assert_eq!(
            config
                .get("responseModalities")
                .and_then(Value::as_array)
                .and_then(|value| value.first())
                .and_then(Value::as_str),
            Some("text")
        );

        let image_config = config
            .get("imageConfig")
            .and_then(Value::as_object)
            .expect("imageConfig must be object");
        assert_eq!(
            image_config.get("imageSize").and_then(Value::as_str),
            Some("image_size_1")
        );
        assert_eq!(
            image_config.get("aspectRatio").and_then(Value::as_str),
            Some("16:9")
        );
    }

    #[test]
    fn vertexai_disables_include_thoughts_when_budget_zero() {
        let payload = json!({
            "model": "gemini-2.5-flash",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 1024,
            "reasoning_effort": "min",
            "include_reasoning": true
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build_vertexai(payload).expect("build should succeed");
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
            thinking.get("thinkingBudget").and_then(Value::as_i64),
            Some(0)
        );
        assert_eq!(
            thinking.get("includeThoughts").and_then(Value::as_bool),
            Some(false)
        );
    }
}
