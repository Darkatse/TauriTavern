use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::super::custom_parameters;
use super::openai;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_raw = payload
        .get("custom_include_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let exclude_raw = payload
        .get("custom_exclude_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let (endpoint, mut upstream_payload) = openai::build(payload);
    apply_custom_body_overrides(&mut upstream_payload, &include_raw, &exclude_raw)?;

    Ok((endpoint, upstream_payload))
}

fn apply_custom_body_overrides(
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

#[cfg(test)]
mod tests {
    use serde_json::Value;
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

    #[test]
    fn custom_payload_supports_nested_yaml_overrides() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "gpt-4.1-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "custom_include_body": "thinking: { type: 'enabled' }\nenable_thinking: true\nchat_template_kwargs: { thinking: true }",
            "custom_url": "http://localhost:1234/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let body = upstream
            .as_object()
            .expect("upstream body should be object");

        assert_eq!(
            body.get("enable_thinking")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            true
        );
        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|object| object.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "enabled"
        );
        assert_eq!(
            body.get("chat_template_kwargs")
                .and_then(Value::as_object)
                .and_then(|object| object.get("thinking"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            true
        );
    }
}
