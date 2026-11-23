use std::borrow::Cow;

use serde_json::{Map, Value};

pub fn format_endpoint(base_url: &str, endpoint_path: &str) -> String {
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

pub fn pretty_json(value: &Value) -> String {
    format!("{value:#}")
}

pub fn extract_model(payload: &Value) -> Option<String> {
    payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Replaces inline media with type/size markers.
pub fn wire_log_payload(payload: Cow<'_, Value>) -> Cow<'_, Value> {
    if !contains_inline_media(&payload) {
        return payload;
    }

    let mut sanitized = payload.into_owned();
    redact_inline_media(&mut sanitized);
    Cow::Owned(sanitized)
}

pub(super) fn parse_media_marker(value: &str) -> Option<(&str, usize)> {
    let body = value
        .strip_prefix("<inline media omitted; mime=")?
        .strip_suffix('>')?;
    let (mime_type, len_str) = body.split_once("; base64_chars=")?;
    if mime_type.is_empty() {
        return None;
    }
    let data_len = len_str.parse::<usize>().ok()?;
    Some((mime_type, data_len))
}

pub(super) fn data_url_media(value: &str) -> Option<(&str, usize)> {
    let body = value.strip_prefix("data:")?;
    let (metadata, data) = body.split_once(',')?;
    let mime_type = metadata.strip_suffix(";base64")?.trim();
    if mime_type.is_empty() || data.is_empty() {
        return None;
    }
    Some((mime_type, data.len()))
}

fn contains_inline_media(value: &Value) -> bool {
    match value {
        Value::String(text) => data_url_media(text).is_some(),
        Value::Array(values) => values.iter().any(contains_inline_media),
        Value::Object(object) => {
            inline_media(object).is_some()
                || input_audio_media(object).is_some()
                || object.values().any(contains_inline_media)
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
            if let Some((mime_type, data_len)) = input_audio_media(object)
                && let Some(input_audio) =
                    object.get_mut("input_audio").and_then(Value::as_object_mut)
            {
                let marker = media_marker(&mime_type, data_len);
                input_audio.insert("data".to_string(), Value::String(marker));
            }
            for value in object.values_mut() {
                redact_inline_media(value);
            }
        }
        _ => {}
    }
}

fn inline_media(object: &Map<String, Value>) -> Option<(&str, usize)> {
    let data = object
        .get("data")?
        .as_str()
        .filter(|data| !data.is_empty())?;
    let mime_type = ["media_type", "mime_type", "mimeType"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))?;
    Some((mime_type, data.len()))
}

fn input_audio_media(object: &Map<String, Value>) -> Option<(String, usize)> {
    let audio = object.get("input_audio")?.as_object()?;
    let data = audio
        .get("data")?
        .as_str()
        .filter(|data| !data.is_empty())?;
    let format = audio
        .get("format")?
        .as_str()
        .map(str::trim)
        .filter(|format| !format.is_empty())?;
    let mime = if format.contains('/') {
        format.to_string()
    } else {
        format!("audio/{format}")
    };
    Some((mime, data.len()))
}

fn media_marker(mime_type: &str, data_len: usize) -> String {
    format!("<inline media omitted; mime={mime_type}; base64_chars={data_len}>")
}
