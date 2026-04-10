use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::super::custom_parameters;

pub(super) fn insert_if_present(dst: &mut Map<String, Value>, src: &Map<String, Value>, key: &str) {
    if let Some(value) = src.get(key).filter(|value| !value.is_null()) {
        dst.insert(key.to_string(), value.clone());
    }
}

pub(super) fn message_content_to_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };

    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    Value::String(fragment) => text.push_str(fragment),
                    Value::Object(object) => {
                        if let Some(fragment) = object.get("text").and_then(Value::as_str) {
                            text.push_str(fragment);
                        } else if let Some(fragment) = object.get("content").and_then(Value::as_str)
                        {
                            text.push_str(fragment);
                        }
                    }
                    _ => {}
                }
            }
            text
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
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

pub(super) fn apply_custom_body_overrides(
    upstream_payload: &mut Value,
    include_raw: &str,
    exclude_raw: &str,
) -> Result<(), ApplicationError> {
    let Some(body) = upstream_payload.as_object_mut() else {
        return Err(ApplicationError::InternalError(
            "Custom upstream payload must be an object".to_string(),
        ));
    };

    if !include_raw.trim().is_empty() {
        let include_map = custom_parameters::parse_object(include_raw)?;
        for (key, value) in include_map {
            body.insert(key, value);
        }
    }

    for key in custom_parameters::parse_key_list(exclude_raw)? {
        body.remove(&key);
    }

    Ok(())
}
