use serde_json::{Map, Value};
use tt_contracts::observability::PartViewDto;

use super::projection::pointer_push;

pub(super) fn collect_visible_reasoning_texts(object: &Map<String, Value>) -> Vec<String> {
    let mut texts = Vec::new();
    for key in [
        "reasoning_content",
        "text",
        "thinking",
        "summary_text",
        "content",
        "summary",
    ] {
        if let Some(value) = object.get(key) {
            push_visible_reasoning_texts(&mut texts, value);
        }
    }
    texts
}

pub(super) fn collect_visible_reasoning_value(value: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    push_visible_reasoning_texts(&mut texts, value);
    texts
}

pub(super) fn collect_native_state_parts(
    object: &Map<String, Value>,
    base_path: Option<&str>,
) -> Vec<PartViewDto> {
    let mut parts = Vec::new();

    for key in ["signature", "encrypted_content", "thoughtSignature"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let length = value_encoded_len(value);
        if length == 0 {
            continue;
        }
        parts.push(PartViewDto::NativeState {
            kind: key.to_string(),
            length,
            path: base_path.map(|base| pointer_push(base, key)),
        });
    }

    if let Some(sig) = object
        .get("extra_content")
        .and_then(Value::as_object)
        .and_then(|extra| extra.get("google"))
        .and_then(Value::as_object)
        .and_then(|google| google.get("thought_signature"))
    {
        let length = value_encoded_len(sig);
        if length > 0 {
            parts.push(PartViewDto::NativeState {
                kind: "thought_signature".to_string(),
                length,
                path: base_path
                    .map(|base| format!("{base}/extra_content/google/thought_signature")),
            });
        }
    }

    parts
}

pub(super) fn value_encoded_len(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::String(text) => text.len(),
        other => other.to_string().len(),
    }
}

fn push_visible_reasoning_texts(texts: &mut Vec<String>, value: &Value) {
    match value {
        Value::String(text) => push_visible_reasoning_text(texts, text),
        Value::Array(items) => {
            for item in items {
                push_visible_reasoning_texts(texts, item);
            }
        }
        Value::Object(object) => {
            for key in [
                "reasoning_content",
                "text",
                "thinking",
                "summary_text",
                "content",
                "summary",
            ] {
                if let Some(value) = object.get(key) {
                    push_visible_reasoning_texts(texts, value);
                }
            }
        }
        _ => {}
    }
}

fn push_visible_reasoning_text(texts: &mut Vec<String>, text: &str) {
    if !text.trim().is_empty() {
        texts.push(text.to_string());
    }
}
