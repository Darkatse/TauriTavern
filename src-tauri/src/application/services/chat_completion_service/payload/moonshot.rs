use serde_json::{Map, Value};

use super::openai;

pub(super) fn build(payload: Map<String, Value>) -> (String, Value) {
    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let (endpoint, mut upstream_payload) = openai::build(payload);

    if endpoint == "/chat/completions" {
        if let Some(body) = upstream_payload.as_object_mut() {
            body.insert(
                "thinking".to_string(),
                serde_json::json!({
                    "type": if include_reasoning { "enabled" } else { "disabled" },
                }),
            );
        }
    }

    (endpoint, upstream_payload)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::build;

    #[test]
    fn moonshot_payload_injects_thinking_flag() {
        let payload = json!({
            "model": "moonshot-v1-8k",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "chat_completion_source": "moonshot"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload);

        assert_eq!(endpoint, "/chat/completions");

        let thinking_type = upstream
            .as_object()
            .and_then(|object| object.get("thinking"))
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        assert_eq!(thinking_type, "enabled");
    }
}
