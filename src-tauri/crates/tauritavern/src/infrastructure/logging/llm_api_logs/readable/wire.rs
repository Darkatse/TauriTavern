use std::borrow::Cow;

use serde_json::Value;

use tt_ports::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionSource,
};

pub(in crate::infrastructure::logging::llm_api_logs) fn format_endpoint(
    base_url: &str,
    endpoint_path: &str,
) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let path = endpoint_path.trim();
    let joined = match (base.is_empty(), path.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base.to_string(),
        (true, false) => path.to_string(),
        (false, false) if path.starts_with('/') => format!("{base}{path}"),
        (false, false) => format!("{base}/{path}"),
    };

    let Ok(mut url) = reqwest::Url::parse(&joined) else {
        return joined;
    };

    let _ = url.set_username("");
    let _ = url.set_password(None);

    let formatted = url.to_string();
    if path.is_empty() {
        formatted.trim_end_matches('/').to_string()
    } else {
        formatted
    }
}

pub(in crate::infrastructure::logging::llm_api_logs) fn extract_model(
    payload: &Value,
) -> Option<String> {
    payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(in crate::infrastructure::logging::llm_api_logs) fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

pub(in crate::infrastructure::logging::llm_api_logs) fn wire_log_payload(
    payload: &Value,
) -> Cow<'_, Value> {
    let has_provider_state = payload
        .as_object()
        .is_some_and(|object| object.contains_key(CHAT_COMPLETION_PROVIDER_STATE_FIELD));
    if !has_provider_state && !contains_inline_media(payload) {
        return Cow::Borrowed(payload);
    }

    let mut sanitized = payload.clone();
    redact_inline_media(&mut sanitized);
    if let Some(object) = sanitized.as_object_mut() {
        object.remove(CHAT_COMPLETION_PROVIDER_STATE_FIELD);
    }
    Cow::Owned(sanitized)
}

fn contains_inline_media(value: &Value) -> bool {
    match value {
        Value::String(text) => data_url_media(text).is_some(),
        Value::Array(values) => values.iter().any(contains_inline_media),
        Value::Object(object) => {
            inline_media(object).is_some() || object.values().any(contains_inline_media)
        }
        _ => false,
    }
}

fn redact_inline_media(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some((mime_type, data_len)) = data_url_media(text) {
                *text = media_marker(mime_type, data_len);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_inline_media(value);
            }
        }
        Value::Object(object) => {
            if let Some((mime_type, data_len)) = inline_media(object) {
                let marker = media_marker(mime_type, data_len);
                object.insert("data".to_string(), Value::String(marker));
            }
            for value in object.values_mut() {
                redact_inline_media(value);
            }
        }
        _ => {}
    }
}

fn inline_media(object: &serde_json::Map<String, Value>) -> Option<(&str, usize)> {
    let data = object
        .get("data")?
        .as_str()
        .filter(|data| !data.is_empty())?;
    let mime_type = ["media_type", "mime_type", "mimeType"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))?;
    Some((mime_type, data.len()))
}

fn data_url_media(value: &str) -> Option<(&str, usize)> {
    let body = value.strip_prefix("data:")?;
    let (metadata, data) = body.split_once(',')?;
    let mime_type = metadata.strip_suffix(";base64")?.trim();
    if mime_type.is_empty() || data.is_empty() {
        return None;
    }
    Some((mime_type, data.len()))
}

fn media_marker(mime_type: &str, data_len: usize) -> String {
    format!("<inline media omitted; mime={mime_type}; base64_chars={data_len}>")
}

pub(in crate::infrastructure::logging::llm_api_logs) fn stream_readable_source(
    source: ChatCompletionSource,
    endpoint_path: &str,
) -> ChatCompletionSource {
    if matches!(
        source,
        ChatCompletionSource::Custom | ChatCompletionSource::OpenCode
    ) && endpoint_path.trim() == "/messages"
    {
        return ChatCompletionSource::Claude;
    }
    if source == ChatCompletionSource::OpenCode
        && matches!(
            endpoint_path.trim(),
            "/generateContent" | "/streamGenerateContent"
        )
    {
        return ChatCompletionSource::Makersuite;
    }

    source
}
