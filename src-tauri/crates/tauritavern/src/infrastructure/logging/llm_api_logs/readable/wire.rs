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
    let Some(object) = payload.as_object() else {
        return Cow::Borrowed(payload);
    };
    if !object.contains_key(CHAT_COMPLETION_PROVIDER_STATE_FIELD) {
        return Cow::Borrowed(payload);
    }

    let mut object = object.clone();
    object.remove(CHAT_COMPLETION_PROVIDER_STATE_FIELD);
    Cow::Owned(Value::Object(object))
}

pub(in crate::infrastructure::logging::llm_api_logs) fn stream_readable_source(
    source: ChatCompletionSource,
    endpoint_path: &str,
) -> ChatCompletionSource {
    if matches!(source, ChatCompletionSource::Custom) && endpoint_path.trim() == "/messages" {
        return ChatCompletionSource::Claude;
    }

    source
}
