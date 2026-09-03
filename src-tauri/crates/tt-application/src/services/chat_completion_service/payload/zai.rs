use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::super::model_capabilities::map_zai_reasoning_effort;
use super::openai;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let reasoning_effort = payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(|value| map_zai_reasoning_effort(model, value))
        .transpose()?
        .flatten();

    let (endpoint, mut upstream_payload) = openai::build(payload)?;

    if endpoint == "/chat/completions"
        && let Some(body) = upstream_payload.as_object_mut()
    {
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

        if body.get("stream").and_then(Value::as_bool) == Some(true)
            && body
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| !tools.is_empty())
        {
            body.insert("tool_stream".to_string(), Value::Bool(true));
        }
    }

    Ok((endpoint, upstream_payload))
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
    fn zai_streaming_tools_enable_tool_stream() {
        let payload = payload(json!({
            "model": "glm-4.6",
            "messages": [{"role": "user", "content": "hello"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write_file",
                    "parameters": {"type": "object"}
                }
            }],
            "stream": true,
            "chat_completion_source": "zai"
        }));

        let (_endpoint, upstream) = build(payload).expect("payload should build");

        assert_eq!(body(&upstream).get("tool_stream"), Some(&Value::Bool(true)));
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
    fn zai_payload_maps_effort_without_requiring_thinking() {
        for (model, effort, expected) in [
            ("glm-5.2", "xhigh", "max"),
            ("glm-5.2", "max", "max"),
            ("glm-5.3", "xhigh", "max"),
            ("glm-5.3", "max", "max"),
            ("glm-5.3", "medium", "medium"),
            ("glm-5.3-flash", "xhigh", "max"),
            ("glm-5.3-flash", "max", "max"),
        ] {
            let payload = payload(json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "include_reasoning": false,
                "reasoning_effort": effort,
                "chat_completion_source": "zai"
            }));

            let (_endpoint, upstream) = build(payload).expect("payload should build");

            assert_eq!(
                body(&upstream)
                    .get("reasoning_effort")
                    .and_then(Value::as_str),
                Some(expected),
                "{model} {effort}"
            );
            assert_eq!(
                body(&upstream)
                    .get("thinking")
                    .and_then(Value::as_object)
                    .and_then(|thinking| thinking.get("type"))
                    .and_then(Value::as_str),
                Some("disabled"),
                "{model} {effort}"
            );
        }
    }
}
