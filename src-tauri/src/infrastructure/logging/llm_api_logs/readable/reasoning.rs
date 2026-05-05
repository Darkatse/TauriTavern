use serde_json::{Map, Value};

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

pub(super) fn has_reasoning_native_state(object: &Map<String, Value>) -> bool {
    object.get("signature").is_some()
        || object.get("encrypted_content").is_some()
        || object.get("thoughtSignature").is_some()
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
    let text = text.trim();
    if !text.is_empty() {
        texts.push(text.to_string());
    }
}
