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

    let enable_image_modality = request_images
        && GOOGLE_IMAGE_GENERATION_MODELS
            .iter()
            .any(|entry| *entry == model);

    let use_system_prompt = payload
        .get("use_sysprompt")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !enable_image_modality
        && !is_gemma;

    let (contents, system_prompt) =
        convert_messages(payload.get("messages"), model, use_system_prompt);

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

    inject_google_thinking_config(payload, model, use_vertex_ai, &mut generation_config);

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
            && !GOOGLE_NO_SEARCH_MODELS.iter().any(|entry| *entry == model)
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
) -> (Vec<Value>, String) {
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

    let model_lower = model.trim().to_ascii_lowercase();
    let supports_signatures =
        model_lower.contains("gemini-3") || model_lower.contains("gemini-2.5");
    let is_gemini3 = model_lower.contains("gemini-3");
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

        let native_gemini_parts = if role == "assistant" {
            message_native_gemini_parts(message)
        } else {
            None
        };
        let mut parts = if let Some(native_parts) = native_gemini_parts.clone() {
            native_parts
        } else {
            convert_message_content_to_parts(message.get("content"), is_gemini3)
        };

        if role == "assistant" {
            let tool_calls = extract_openai_tool_calls(message.get("tool_calls"));
            if !tool_calls.is_empty() {
                for tool_call in &tool_calls {
                    tool_name_by_id.insert(tool_call.id.clone(), tool_call.name.clone());
                }
                if native_gemini_parts.is_none() {
                    parts.extend(convert_openai_tool_calls_to_parts(&tool_calls));
                }
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

                if let Some(text_signature) = text_signature {
                    if is_text_part {
                        part_object.insert(
                            "thoughtSignature".to_string(),
                            Value::String(text_signature.to_string()),
                        );
                        continue;
                    }
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

                    if is_image_model && target_role == "model" {
                        if is_text_part || part_object.get("inlineData").is_some() {
                            part_object.insert(
                                "thoughtSignature".to_string(),
                                Value::String(skip_signature_magic.to_string()),
                            );
                        }
                    }
                }
            }
        }

        contents.push(json!({
            "role": target_role,
            "parts": parts,
        }));
    }

    (contents, system_parts.join("\n\n"))
}

fn convert_message_content_to_parts(content: Option<&Value>, is_gemini3: bool) -> Vec<Value> {
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
                        let image_url = object.get("image_url").and_then(Value::as_object);
                        let data_url = image_url
                            .and_then(|entry| entry.get("url"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty());
                        let detail = image_url
                            .and_then(|entry| entry.get("detail"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty());

                        if let Some(data_url) = data_url {
                            if let Some((mime_type, data)) = parse_data_url(data_url) {
                                let mut part = json!({
                                    "inlineData": {
                                        "mimeType": mime_type,
                                        "data": data,
                                    }
                                });

                                if is_gemini3 {
                                    if let Some(level) = detail.and_then(gemini_media_resolution) {
                                        if let Some(part_object) = part.as_object_mut() {
                                            part_object.insert(
                                                "mediaResolution".to_string(),
                                                json!({ "level": level }),
                                            );
                                        }
                                    }
                                }

                                return Some(part);
                            }
                        }
                    }

                    if object
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == "video_url")
                    {
                        let video_url = object.get("video_url").and_then(Value::as_object);
                        let data_url = video_url
                            .and_then(|entry| entry.get("url"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty());
                        let detail = video_url
                            .and_then(|entry| entry.get("detail"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty());

                        if let Some(data_url) = data_url {
                            if let Some((mime_type, data)) = parse_data_url(data_url) {
                                let mut part = json!({
                                    "inlineData": {
                                        "mimeType": mime_type,
                                        "data": data,
                                    }
                                });

                                if is_gemini3 {
                                    if let Some(level) = detail.and_then(gemini_media_resolution) {
                                        if let Some(part_object) = part.as_object_mut() {
                                            part_object.insert(
                                                "mediaResolution".to_string(),
                                                json!({ "level": level }),
                                            );
                                        }
                                    }
                                }

                                return Some(part);
                            }
                        }
                    }

                    if object
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value == "audio_url")
                    {
                        let data_url = object
                            .get("audio_url")
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

fn message_native_gemini_parts(message: &Map<String, Value>) -> Option<Vec<Value>> {
    message
        .get("native")?
        .get("gemini")?
        .get("content")?
        .get("parts")?
        .as_array()
        .cloned()
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
    let mut include_thoughts = include_reasoning;

    if let Some(budget) = calculate_google_budget_tokens(max_output_tokens, reasoning_effort, model)
    {
        match budget {
            GoogleThinkingBudget::Tokens(tokens) => {
                thinking_config.insert(
                    "thinkingBudget".to_string(),
                    Value::Number(serde_json::Number::from(tokens)),
                );

                if use_vertex_ai && tokens == 0 && include_thoughts {
                    include_thoughts = false;
                }
            }
            GoogleThinkingBudget::Level(level) => {
                thinking_config.insert(
                    "thinkingLevel".to_string(),
                    Value::String(level.to_string()),
                );
            }
        }
    }

    thinking_config.insert("includeThoughts".to_string(), Value::Bool(include_thoughts));

    generation_config.insert("thinkingConfig".to_string(), Value::Object(thinking_config));
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

    use super::{build, build_vertexai};

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
