use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::super::custom_parameters;
use super::openai;

pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    apply_custom_body_overrides(&mut payload)?;
    Ok(openai::build(payload))
}

fn apply_custom_body_overrides(payload: &mut Map<String, Value>) -> Result<(), ApplicationError> {
    let include_raw = payload
        .get("custom_include_body")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if !include_raw.trim().is_empty() {
        let include_map = custom_parameters::parse_object(include_raw)?;
        for (key, value) in include_map {
            payload.insert(key, value);
        }
    }

    let exclude_raw = payload
        .get("custom_exclude_body")
        .and_then(Value::as_str)
        .unwrap_or_default();

    for key in custom_parameters::parse_key_list(exclude_raw) {
        payload.remove(&key);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build;

    #[test]
    fn custom_payload_applies_overrides_and_strips_internal_fields() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "gpt-4.1-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.1,
            "custom_include_body": "{\"temperature\":0.7,\"presence_penalty\":0.2}",
            "custom_exclude_body": "[\"messages\"]",
            "custom_include_headers": "{\"x-test\":\"1\"}",
            "custom_url": "http://localhost:1234/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");

        assert_eq!(endpoint, "/chat/completions");

        let body = upstream
            .as_object()
            .expect("upstream body should be object");
        assert_eq!(
            body.get("temperature")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default(),
            0.7
        );
        assert_eq!(
            body.get("presence_penalty")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default(),
            0.2
        );
        assert!(body.get("messages").is_none());
        assert!(body.get("custom_include_body").is_none());
        assert!(body.get("custom_exclude_body").is_none());
        assert!(body.get("custom_include_headers").is_none());
        assert!(body.get("custom_url").is_none());
    }
}
