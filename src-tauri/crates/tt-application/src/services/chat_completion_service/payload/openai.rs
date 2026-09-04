use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::content_parts::{
    AudioInputFormatSet, AudioRewriteFallbackPolicy, reject_media_for_text_only_provider,
    reject_video_for_provider, rewrite_audio_url_parts_to_input_audio,
};
use super::openai_reasoning::{
    normalize_openai_reasoning_effort, should_forward_openai_reasoning_effort,
};
use super::shared::{insert_if_present, message_content_to_text};

const TEXT_COMPLETION_MODELS: &[&str] = &[
    "gpt-3.5-turbo-instruct",
    "gpt-3.5-turbo-instruct-0914",
    "text-davinci-003",
    "text-davinci-002",
    "text-davinci-001",
    "text-curie-001",
    "text-babbage-001",
    "text-ada-001",
    "code-davinci-002",
    "code-davinci-001",
    "code-cushman-002",
    "code-cushman-001",
    "text-davinci-edit-001",
    "code-davinci-edit-001",
    "text-embedding-ada-002",
    "text-similarity-davinci-001",
    "text-similarity-curie-001",
    "text-similarity-babbage-001",
    "text-similarity-ada-001",
    "text-search-davinci-doc-001",
    "text-search-curie-doc-001",
    "text-search-babbage-doc-001",
    "text-search-ada-doc-001",
    "code-search-babbage-code-001",
    "code-search-ada-code-001",
];

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    build_with_text_completions(payload, true)
}

pub(super) fn build_chat(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    build_with_text_completions(payload, false)
}

fn build_with_text_completions(
    payload: Map<String, Value>,
    allow_text_completions: bool,
) -> Result<(String, Value), ApplicationError> {
    let mut payload = payload;
    let source = payload
        .get("chat_completion_source")
        .and_then(Value::as_str)
        .unwrap_or("openai")
        .trim()
        .to_ascii_lowercase();
    strip_internal_fields(&mut payload);
    build_clean(payload, &source, allow_text_completions)
}

pub(super) fn strip_internal_fields(payload: &mut Map<String, Value>) {
    for key in [
        "chat_completion_source",
        "reverse_proxy",
        "proxy_password",
        "custom_api_format",
        "custom_prompt_post_processing",
        "custom_include_body",
        "custom_exclude_body",
        "custom_include_headers",
        "custom_claude_prompt_caching",
        "custom_openai_responses_websocket",
        "custom_url",
        "secret_id",
        "bypass_status_check",
        "siliconflow_endpoint",
        "minimax_endpoint",
        "moonshot_endpoint",
        "workers_ai_account_id",
        "nanogpt_provider",
        "nanogpt_payg_override",
    ] {
        payload.remove(key);
    }
}

fn build_clean(
    payload: Map<String, Value>,
    source: &str,
    allow_text_completions: bool,
) -> Result<(String, Value), ApplicationError> {
    if allow_text_completions && is_text_completion(&payload) {
        Ok((
            "/completions".to_string(),
            Value::Object(build_text_completion_payload(&payload)?),
        ))
    } else {
        Ok((
            "/chat/completions".to_string(),
            Value::Object(build_chat_completion_payload(&payload, source)?),
        ))
    }
}

fn build_text_completion_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let mut request = Map::new();

    for key in [
        "model",
        "temperature",
        "max_tokens",
        "stream",
        "presence_penalty",
        "frequency_penalty",
        "top_p",
        "stop",
        "logit_bias",
        "seed",
        "n",
        "logprobs",
    ] {
        insert_if_present(&mut request, payload, key);
    }

    if let Some(prompt) = payload
        .get("prompt")
        .cloned()
        .filter(|value| !value.is_null())
    {
        request.insert("prompt".to_string(), prompt);
        return Ok(request);
    }

    if let Some(messages) = payload.get("messages")
        && let Some(prompt) = convert_text_completion_prompt(messages)?
    {
        request.insert("prompt".to_string(), Value::String(prompt));
    }

    Ok(request)
}

fn build_chat_completion_payload(
    payload: &Map<String, Value>,
    source: &str,
) -> Result<Map<String, Value>, ApplicationError> {
    let mut request = Map::new();

    for key in [
        "messages",
        "model",
        "temperature",
        "max_tokens",
        "max_completion_tokens",
        "stream",
        "presence_penalty",
        "frequency_penalty",
        "top_p",
        "top_k",
        "stop",
        "logit_bias",
        "seed",
        "n",
        "user",
    ] {
        insert_if_present(&mut request, payload, key);
    }

    if source == "custom"
        && let Some(reasoning_effort) = payload.get("reasoning_effort")
    {
        request.insert("reasoning_effort".to_string(), reasoning_effort.clone());
    }

    if let Some(model) = payload.get("model").and_then(Value::as_str) {
        if should_forward_openai_reasoning_effort(source, model)
            && let Some(reasoning_effort) = payload
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .and_then(|value| normalize_openai_reasoning_effort(value, model))
        {
            request.insert(
                "reasoning_effort".to_string(),
                Value::String(reasoning_effort.to_owned()),
            );
        }

        if should_forward_openai_verbosity(source, model)
            && let Some(verbosity) = payload
                .get("verbosity")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        {
            request.insert(
                "verbosity".to_string(),
                Value::String(verbosity.to_string()),
            );
        }
    }

    if let Some(tools) = payload.get("tools").filter(|value| value.is_array()) {
        request.insert("tools".to_string(), tools.clone());
        insert_if_present(&mut request, payload, "tool_choice");
    }

    map_chat_logprobs(&mut request, payload);

    if let Some(response_format) = resolve_response_format(payload) {
        request.insert("response_format".to_string(), response_format);
    }

    if source == "openai" {
        rewrite_audio_url_parts_to_input_audio(
            "OpenAI Chat Completions",
            request.get_mut("messages"),
            AudioInputFormatSet::OpenAi,
            AudioRewriteFallbackPolicy::Reject,
        )?;
        reject_video_for_provider("OpenAI Chat Completions", request.get("messages"))?;
    }

    Ok(request)
}

fn should_forward_openai_verbosity(source: &str, model: &str) -> bool {
    matches!(source, "openai" | "custom") && model.trim().to_ascii_lowercase().starts_with("gpt-5")
}

fn map_chat_logprobs(request: &mut Map<String, Value>, payload: &Map<String, Value>) {
    let Some(logprobs) = payload.get("logprobs") else {
        return;
    };

    if let Some(raw_number) = logprobs.as_i64() {
        if raw_number > 0 {
            request.insert("logprobs".to_string(), Value::Bool(true));
            request.insert(
                "top_logprobs".to_string(),
                Value::Number(serde_json::Number::from(raw_number)),
            );
        }
        return;
    }

    if let Some(raw_number) = logprobs.as_u64() {
        if raw_number > 0 {
            request.insert("logprobs".to_string(), Value::Bool(true));
            request.insert(
                "top_logprobs".to_string(),
                Value::Number(serde_json::Number::from(raw_number)),
            );
        }
        return;
    }

    if let Some(raw_number) = logprobs.as_f64() {
        if raw_number > 0.0 {
            request.insert("logprobs".to_string(), Value::Bool(true));
            if let Some(number) = serde_json::Number::from_f64(raw_number) {
                request.insert("top_logprobs".to_string(), Value::Number(number));
            }
        }
        return;
    }

    if let Some(enabled) = logprobs.as_bool() {
        request.insert("logprobs".to_string(), Value::Bool(enabled));
        if enabled {
            insert_if_present(request, payload, "top_logprobs");
        }
    }
}

fn resolve_response_format(payload: &Map<String, Value>) -> Option<Value> {
    payload
        .get("response_format")
        .cloned()
        .filter(|value| !value.is_null())
        .or_else(|| build_response_format_from_json_schema(payload))
}

fn build_response_format_from_json_schema(payload: &Map<String, Value>) -> Option<Value> {
    let json_schema = payload.get("json_schema")?.as_object()?;
    let schema_value = json_schema.get("value")?.clone();
    if schema_value.is_null() {
        return None;
    }

    let mut json_schema_object = Map::new();
    json_schema_object.insert(
        "name".to_string(),
        json_schema
            .get("name")
            .cloned()
            .unwrap_or_else(|| Value::String("response".to_string())),
    );
    json_schema_object.insert(
        "strict".to_string(),
        json_schema
            .get("strict")
            .cloned()
            .unwrap_or(Value::Bool(true)),
    );
    json_schema_object.insert("schema".to_string(), schema_value);

    let mut response_format = Map::new();
    response_format.insert("type".to_string(), Value::String("json_schema".to_string()));
    response_format.insert("json_schema".to_string(), Value::Object(json_schema_object));

    Some(Value::Object(response_format))
}

fn convert_text_completion_prompt(messages: &Value) -> Result<Option<String>, ApplicationError> {
    reject_media_for_text_only_provider("OpenAI-compatible text completions", Some(messages))?;

    if let Some(prompt) = messages.as_str() {
        return Ok(Some(prompt.to_string()));
    }

    let Some(entries) = messages.as_array() else {
        return Ok(None);
    };
    if entries.is_empty() {
        return Ok(None);
    }

    let mut lines = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim();
        let name = message.get("name").and_then(Value::as_str).map(str::trim);
        let content = message_content_to_text(message.get("content"));

        if role.eq_ignore_ascii_case("system") {
            match name {
                Some(value) if !value.is_empty() => {
                    lines.push(format!("{value}: {content}"));
                }
                _ => {
                    lines.push(format!("System: {content}"));
                }
            }
        } else {
            lines.push(format!("{role}: {content}"));
        }
    }

    if lines.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("{}\nassistant:", lines.join("\n"))))
    }
}

fn is_text_completion(payload: &Map<String, Value>) -> bool {
    let messages_is_string = payload.get("messages").is_some_and(Value::is_string);
    if messages_is_string {
        return true;
    }

    payload
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|model| TEXT_COMPLETION_MODELS.contains(&model))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{build, strip_internal_fields};

    #[test]
    fn strip_internal_fields_removes_internal_selectors() {
        let mut payload = json!({
            "secret_id": "profile-secret",
            "moonshot_endpoint": "cn",
            "model": "gpt-4.1-mini"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        strip_internal_fields(&mut payload);

        assert!(payload.get("secret_id").is_none());
        assert!(payload.get("moonshot_endpoint").is_none());
        assert_eq!(
            payload.get("model").and_then(Value::as_str),
            Some("gpt-4.1-mini")
        );
    }

    #[test]
    fn custom_payload_forwards_reasoning_effort_for_unknown_models() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "custom-reasoning-model",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "high",
            "verbosity": "high"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/chat/completions");

        let body = upstream.as_object().expect("payload must be object");

        assert_eq!(
            body.get("reasoning_effort").and_then(Value::as_str),
            Some("high")
        );

        assert!(body.get("verbosity").is_none());
    }

    #[test]
    fn custom_payload_preserves_reasoning_effort_for_openai_model_names() {
        for reasoning_effort in ["min", "max", "xhigh", "auto"] {
            let payload = json!({
                "chat_completion_source": "custom",
                "model": "gpt-5.1",
                "messages": [{"role": "user", "content": "hello"}],
                "reasoning_effort": reasoning_effort
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let (_endpoint, upstream) = build(payload).expect("build should succeed");
            assert_eq!(upstream["reasoning_effort"], reasoning_effort);
        }
    }

    #[test]
    fn non_custom_sources_do_not_forward_reasoning_effort_or_verbosity() {
        let payload = json!({
            "chat_completion_source": "openrouter",
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "high",
            "verbosity": "high"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("payload must be object");
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("verbosity").is_none());
    }

    #[test]
    fn text_completion_rejects_media_parts() {
        let payload = json!({
            "chat_completion_source": "openai",
            "model": "gpt-3.5-turbo-instruct",
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

        let error = build(payload).expect_err("text completions should reject media");
        assert!(error.to_string().contains("cannot preserve image input"));
    }

    #[test]
    fn openai_chat_rewrites_audio_url_data_url_to_input_audio() {
        let payload = json!({
            "chat_completion_source": "openai",
            "model": "gpt-4o-audio-preview",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "transcribe" },
                    { "type": "audio_url", "audio_url": { "url": "data:audio/wave;base64,AAAA" } }
                ]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");

        assert_eq!(
            upstream.pointer("/messages/0/content/1/type"),
            Some(&json!("input_audio"))
        );
        assert_eq!(
            upstream.pointer("/messages/0/content/1/input_audio/format"),
            Some(&json!("wav"))
        );
        assert_eq!(
            upstream.pointer("/messages/0/content/1/input_audio/data"),
            Some(&json!("AAAA"))
        );
        assert!(
            upstream
                .pointer("/messages/0/content/1/audio_url")
                .is_none()
        );
    }

    #[test]
    fn openai_chat_preserves_preformatted_input_audio() {
        let payload = json!({
            "chat_completion_source": "openai",
            "model": "gpt-4o-audio-preview",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "input_audio", "input_audio": { "format": "wav", "data": "AAAA" } }
                ]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");

        assert_eq!(
            upstream.pointer("/messages/0/content/0/type"),
            Some(&json!("input_audio"))
        );
        assert_eq!(
            upstream.pointer("/messages/0/content/0/input_audio/data"),
            Some(&json!("AAAA"))
        );
    }

    #[test]
    fn openai_chat_preserves_image_url_parts() {
        let payload = json!({
            "chat_completion_source": "openai",
            "model": "gpt-4.1-mini",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "describe" },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.test/cat.png",
                            "detail": "high"
                        }
                    }
                ]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");

        assert_eq!(
            upstream.pointer("/messages/0/content/1/image_url/url"),
            Some(&json!("https://example.test/cat.png"))
        );
        assert_eq!(
            upstream.pointer("/messages/0/content/1/image_url/detail"),
            Some(&json!("high"))
        );
    }

    #[test]
    fn openai_chat_rejects_remote_audio_url() {
        let payload = json!({
            "chat_completion_source": "openai",
            "model": "gpt-4o-audio-preview",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "audio_url", "audio_url": { "url": "https://example.test/audio.wav" } },
                    { "type": "audio_url", "audio_url": { "url": "data:audio/wav;base64,AAAA" } }
                ]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("remote audio should fail fast");

        assert!(
            error
                .to_string()
                .contains("remote audio URLs are not supported")
        );
    }

    #[test]
    fn openai_chat_rejects_unsupported_audio_data_url_mime() {
        let payload = json!({
            "chat_completion_source": "openai",
            "model": "gpt-4o-audio-preview",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "audio_url", "audio_url": { "url": "data:audio/webm;base64,AAAA" } }
                ]
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("unsupported MIME should fail fast");

        assert!(
            error
                .to_string()
                .contains("unsupported audio data URL MIME type")
        );
    }

    #[test]
    fn openai_chat_rejects_video_parts() {
        for content in [
            json!([
                { "type": "video_url", "video_url": { "url": "data:video/mp4;base64,AAAA" } }
            ]),
            json!([
                { "type": "video_url", "video_url": { "url": "https://example.test/video.mp4" } }
            ]),
            json!([
                { "inlineData": { "mimeType": "video/mp4", "data": "AAAA" } }
            ]),
            json!({ "type": "video_url", "video_url": { "url": "data:video/mp4;base64,AAAA" } }),
        ] {
            let payload = json!({
                "chat_completion_source": "openai",
                "model": "gpt-4.1-mini",
                "messages": [{
                    "role": "user",
                    "content": content
                }]
            })
            .as_object()
            .cloned()
            .expect("payload must be object");

            let error = build(payload).expect_err("video should fail fast");

            assert!(error.to_string().contains("cannot preserve video input"));
        }
    }
}
