use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::super::model_capabilities::{
    VersionedModel, map_low_high_max_reasoning_effort, map_zai_reasoning_effort,
};
use super::openai;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let version = payload
        .get("model")
        .and_then(Value::as_str)
        .and_then(|model| VersionedModel::parse(model, "glm-"))
        .map(|glm| glm.version);
    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reasoning_effort = payload.get("reasoning_effort").and_then(Value::as_str);
    let reasoning_effort = match version {
        Some((5, 3)) => reasoning_effort
            .map(|value| map_low_high_max_reasoning_effort(value, "Z.AI"))
            .transpose()?
            .flatten(),
        Some(version) if version >= (5, 2) => reasoning_effort
            .map(map_zai_reasoning_effort)
            .transpose()?
            .flatten(),
        _ => None,
    };
    let always_thinks = version == Some((5, 3));

    let (endpoint, mut upstream_payload) = openai::build(payload)?;

    if endpoint == "/chat/completions"
        && let Some(body) = upstream_payload.as_object_mut()
    {
        if !always_thinks {
            body.insert(
                "thinking".to_string(),
                serde_json::json!({
                    "type": if include_reasoning { "enabled" } else { "disabled" },
                }),
            );
        }

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
    fn zai_sends_the_thinking_controls_each_glm_model_accepts() {
        for (model, effort, thinking, expected_effort) in [
            ("glm-5.3-flashx-legal", "medium", None, Some("high")),
            ("glm-5.3", "xhigh", None, Some("high")),
            ("glm-5.3", "min", None, Some("low")),
            ("glm-5.2", "xhigh", Some("disabled"), Some("max")),
            ("glm-5.2", "medium", Some("disabled"), Some("medium")),
            ("glm-4.6", "high", Some("disabled"), None),
        ] {
            let payload = payload(json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "include_reasoning": false,
                "reasoning_effort": effort,
                "chat_completion_source": "zai"
            }));

            let (_endpoint, upstream) = build(payload).expect("payload should build");
            let body = body(&upstream);

            assert_eq!(
                body.get("thinking")
                    .and_then(|thinking| thinking.get("type"))
                    .and_then(Value::as_str),
                thinking,
                "{model} {effort}"
            );
            assert_eq!(
                body.get("reasoning_effort").and_then(Value::as_str),
                expected_effort,
                "{model} {effort}"
            );
        }
    }
}
