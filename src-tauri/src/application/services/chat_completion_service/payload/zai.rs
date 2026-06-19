use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::super::model_capabilities::{
    RequestedReasoningEffort, map_zai_reasoning_effort, parse_known_reasoning_effort,
};
use super::openai;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let reasoning_effort = payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(|value| resolve_reasoning_effort(&model, value, include_reasoning))
        .transpose()?
        .flatten();

    let (endpoint, mut upstream_payload) = openai::build(payload);

    if endpoint == "/chat/completions" {
        if let Some(body) = upstream_payload.as_object_mut() {
            body.insert(
                "thinking".to_string(),
                serde_json::json!({
                    "type": if include_reasoning { "enabled" } else { "disabled" },
                }),
            );

            if let Some(reasoning_effort) = reasoning_effort {
                body.insert(
                    "reasoning_effort".to_string(),
                    Value::String(reasoning_effort.to_string()),
                );
            }
        }
    }

    Ok((endpoint, upstream_payload))
}

fn resolve_reasoning_effort(
    model: &str,
    value: &str,
    include_reasoning: bool,
) -> Result<Option<&'static str>, ApplicationError> {
    if include_reasoning {
        return map_zai_reasoning_effort(model, value);
    }

    match parse_known_reasoning_effort(value, "Z.AI")? {
        RequestedReasoningEffort::Auto => Ok(None),
        _ => Err(ApplicationError::ValidationError(
            "Z.AI reasoning_effort requires include_reasoning=true".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::build;

    fn payload(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("payload must be object")
    }

    fn body(value: &Value) -> &Map<String, Value> {
        value.as_object().expect("upstream body should be object")
    }

    #[test]
    fn zai_payload_injects_thinking_flag() {
        let payload = payload(json!({
            "model": "glm-4.6",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "chat_completion_source": "zai"
        }));

        let (endpoint, upstream) = build(payload).expect("payload should build");

        assert_eq!(endpoint, "/chat/completions");

        let thinking_type = body(&upstream)
            .get("thinking")
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        assert_eq!(thinking_type, "enabled");
    }

    #[test]
    fn zai_payload_disables_thinking_without_reasoning_effort() {
        let payload = payload(json!({
            "model": "glm-5.2",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": false,
            "chat_completion_source": "zai"
        }));

        let (_endpoint, upstream) = build(payload).expect("payload should build");
        let body = body(&upstream);

        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("disabled")
        );
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn zai_glm52_forwards_native_reasoning_effort() {
        let payload = payload(json!({
            "model": "glm-5.2",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "max",
            "chat_completion_source": "zai"
        }));

        let (_endpoint, upstream) = build(payload).expect("payload should build");

        assert_eq!(
            body(&upstream)
                .get("reasoning_effort")
                .and_then(Value::as_str),
            Some("max")
        );
    }

    #[test]
    fn zai_glm52_maps_minimum_to_minimal() {
        let payload = payload(json!({
            "model": "glm-5.2",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "min",
            "chat_completion_source": "zai"
        }));

        let (_endpoint, upstream) = build(payload).expect("payload should build");

        assert_eq!(
            body(&upstream)
                .get("reasoning_effort")
                .and_then(Value::as_str),
            Some("minimal")
        );
    }

    #[test]
    fn zai_glm52_omits_auto_reasoning_effort() {
        let payload = payload(json!({
            "model": "glm-5.2",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "auto",
            "chat_completion_source": "zai"
        }));

        let (_endpoint, upstream) = build(payload).expect("payload should build");

        assert!(body(&upstream).get("reasoning_effort").is_none());
    }

    #[test]
    fn zai_rejects_reasoning_effort_for_unsupported_models() {
        let payload = payload(json!({
            "model": "glm-5.1",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "high",
            "chat_completion_source": "zai"
        }));

        let error = build(payload).expect_err("unsupported model should fail");

        assert!(
            error
                .to_string()
                .contains("Z.AI reasoning_effort is only supported by glm-5.2")
        );
    }

    #[test]
    fn zai_rejects_reasoning_effort_when_thinking_is_disabled() {
        let payload = payload(json!({
            "model": "glm-5.2",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": false,
            "reasoning_effort": "high",
            "chat_completion_source": "zai"
        }));

        let error = build(payload).expect_err("contradictory reasoning request should fail");

        assert!(
            error
                .to_string()
                .contains("Z.AI reasoning_effort requires include_reasoning=true")
        );
    }
}
