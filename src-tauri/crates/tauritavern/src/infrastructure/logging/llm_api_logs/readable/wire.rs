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
    if !has_provider_state && !contains_inline_media(payload, None) {
        return Cow::Borrowed(payload);
    }

    let mut sanitized = payload.clone();
    redact_inline_media(&mut sanitized, None);
    if let Some(object) = sanitized.as_object_mut() {
        object.remove(CHAT_COMPLETION_PROVIDER_STATE_FIELD);
    }
    Cow::Owned(sanitized)
}

fn contains_inline_media(value: &Value, parent_key: Option<&str>) -> bool {
    match value {
        Value::String(text) => data_url_media(text).is_some(),
        Value::Array(values) => values
            .iter()
            .any(|value| contains_inline_media(value, parent_key)),
        Value::Object(object) => {
            if is_base64_media_object(object, parent_key)
                && object
                    .get("data")
                    .and_then(Value::as_str)
                    .is_some_and(|data| !data.is_empty())
            {
                return true;
            }

            object
                .iter()
                .any(|(key, value)| contains_inline_media(value, Some(key)))
        }
        _ => false,
    }
}

fn redact_inline_media(value: &mut Value, parent_key: Option<&str>) {
    match value {
        Value::String(text) => {
            if let Some((mime_type, data_len)) = data_url_media(text) {
                *text = media_marker(mime_type, data_len);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_inline_media(value, parent_key);
            }
        }
        Value::Object(object) => {
            if is_base64_media_object(object, parent_key) {
                let mime_type = object
                    .get("media_type")
                    .or_else(|| object.get("mimeType"))
                    .or_else(|| object.get("mime_type"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                if let Some(data_len) = object
                    .get("data")
                    .and_then(Value::as_str)
                    .map(str::len)
                    .filter(|length| *length > 0)
                {
                    object.insert(
                        "data".to_string(),
                        Value::String(media_marker(mime_type, data_len)),
                    );
                }
            }

            for (key, value) in object {
                redact_inline_media(value, Some(key));
            }
        }
        _ => {}
    }
}

fn is_base64_media_object(
    object: &serde_json::Map<String, Value>,
    parent_key: Option<&str>,
) -> bool {
    object.get("type").and_then(Value::as_str) == Some("base64")
        || matches!(parent_key, Some("inlineData" | "inline_data"))
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
