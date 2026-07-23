use serde_json::{Map, Value};

use super::content_parts::openai_chat_content_to_lossy_text;

pub(super) fn add_assistant_prefix(messages: &mut [Value], property: &str) {
    let Some(last_message) = messages.last_mut().and_then(Value::as_object_mut) else {
        return;
    };

    if last_message.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }

    last_message.insert(property.to_string(), Value::Bool(true));
}

pub(super) fn insert_if_present(dst: &mut Map<String, Value>, src: &Map<String, Value>, key: &str) {
    if let Some(value) = src.get(key).filter(|value| !value.is_null()) {
        dst.insert(key.to_string(), value.clone());
    }
}

pub(super) fn message_content_to_text(content: Option<&Value>) -> String {
    openai_chat_content_to_lossy_text(content)
}

pub(super) fn parse_data_url(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    let body = trimmed.strip_prefix("data:")?;
    let (mime_and_encoding, data) = body.split_once(',')?;
    let (mime_type, encoding) = mime_and_encoding.split_once(';')?;

    if encoding != "base64" || mime_type.trim().is_empty() || data.trim().is_empty() {
        return None;
    }

    Some((mime_type.trim().to_string(), data.trim().to_string()))
}
