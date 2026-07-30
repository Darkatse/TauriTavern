use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::openai;
use super::shared::add_assistant_prefix;

pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reasoning_effort = payload.remove("reasoning_effort");
    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        add_assistant_prefix(messages, "partial");
    }

    let (endpoint, mut upstream_payload) = openai::build(payload)?;

    if endpoint == "/chat/completions"
        && let Some(body) = upstream_payload.as_object_mut()
    {
        if let Some(reasoning_effort) = reasoning_effort {
            body.insert("reasoning_effort".to_string(), reasoning_effort);
        }
        body.insert(
            "thinking".to_string(),
            serde_json::json!({
                "type": if include_reasoning { "enabled" } else { "disabled" },
            }),
        );
    }

    Ok((endpoint, upstream_payload))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn moonshot_payload_preserves_user_parameters_and_injects_thinking_flag() {
        let payload = json!({
            "model": "kimi-k3",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "max",
            "temperature": 0.42,
            "top_p": 0.88,
            "frequency_penalty": 0.1,
            "n": 2,
            "chat_completion_source": "moonshot"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");

        assert_eq!(endpoint, "/chat/completions");

        let thinking_type = upstream
            .as_object()
            .and_then(|object| object.get("thinking"))
            .and_then(Value::as_object)
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        assert_eq!(thinking_type, "enabled");
        assert_eq!(upstream.get("temperature"), Some(&json!(0.42)));
        assert_eq!(upstream.get("top_p"), Some(&json!(0.88)));
        assert_eq!(upstream.get("frequency_penalty"), Some(&json!(0.1)));
        assert_eq!(upstream.get("n"), Some(&json!(2)));
        assert_eq!(upstream.get("reasoning_effort"), Some(&json!("max")));
    }

    #[test]
    fn moonshot_marks_assistant_prefill_as_partial() {
        let payload = json!({
            "model": "kimi-k3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "prefill"}
            ],
            "chat_completion_source": "moonshot"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let partial = upstream
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .and_then(|message| message.get("partial"))
            .and_then(Value::as_bool);

        assert_eq!(partial, Some(true));
    }

    #[test]
    fn moonshot_partial_coexists_with_tools() {
        let payload = json!({
            "model": "kimi-k3",
            "messages": [
                {"role": "user", "content": "look it up"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "lookup", "arguments": "{}"}
                    }]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "result"},
                {"role": "assistant", "content": "prefill"}
            ],
            "tools": [{
                "type": "function",
                "function": {"name": "lookup", "parameters": {"type": "object"}}
            }],
            "chat_completion_source": "moonshot"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let partial = upstream
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .and_then(|message| message.get("partial"))
            .and_then(Value::as_bool);

        assert_eq!(partial, Some(true));
    }
}
