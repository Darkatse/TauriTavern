use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::openai;

pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_reasoning = payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reasoning_effort = payload.remove("reasoning_effort");

    let has_json_schema = payload.contains_key("json_schema");
    let has_tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());

    if !has_json_schema && !has_tools {
        if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
            let has_tool_message = messages
                .iter()
                .any(|message| message.get("role").and_then(Value::as_str) == Some("tool"));

            if !has_tool_message {
                if let Some(last_message) = messages.last_mut().and_then(Value::as_object_mut) {
                    if last_message.get("role").and_then(Value::as_str) == Some("assistant") {
                        if let Some(content) = last_message
                            .get("content")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                        {
                            let trimmed = content.trim_start();

                            if let Some(after_open) = trimmed.strip_prefix("<think>") {
                                let (reasoning, remainder) = match after_open.split_once("</think>")
                                {
                                    Some((reasoning, remainder)) => {
                                        (reasoning, remainder.trim_start().to_string())
                                    }
                                    None => (after_open, String::new()),
                                };

                                last_message.insert(
                                    "reasoning_content".to_string(),
                                    Value::String(reasoning.trim().to_string()),
                                );
                                last_message
                                    .insert("content".to_string(), Value::String(remainder));
                            }
                        }

                        last_message.insert("partial".to_string(), Value::Bool(true));
                    }
                }
            }
        }
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

    fn build_payload(payload: Value) -> Value {
        let payload = payload
            .as_object()
            .cloned()
            .expect("payload must be an object");
        build(payload).expect("build should succeed").1
    }

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
        });

        let payload = payload
            .as_object()
            .cloned()
            .expect("payload must be object");
        let (endpoint, upstream) = build(payload).expect("build should succeed");

        assert_eq!(endpoint, "/chat/completions");
        assert_eq!(upstream["thinking"]["type"], "enabled");
        assert_eq!(upstream["temperature"], json!(0.42));
        assert_eq!(upstream["top_p"], json!(0.88));
        assert_eq!(upstream["frequency_penalty"], json!(0.1));
        assert_eq!(upstream["n"], json!(2));
        assert_eq!(upstream["reasoning_effort"], json!("max"));
    }

    #[test]
    fn moonshot_marks_assistant_prefill_as_partial() {
        let upstream = build_payload(json!({
            "model": "kimi-k3",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "prefill"}
            ]
        }));

        assert_eq!(upstream["messages"][1]["partial"], json!(true));
    }

    #[test]
    fn moonshot_moves_closed_think_prefill_into_reasoning_content() {
        let upstream = build_payload(json!({
            "model": "kimi-k3",
            "messages": [
                {"role": "assistant", "content": "  <think>reasoning here</think> answer"}
            ]
        }));

        assert_eq!(
            upstream["messages"][0]["reasoning_content"],
            json!("reasoning here")
        );
        assert_eq!(upstream["messages"][0]["content"], json!("answer"));
        assert_eq!(upstream["messages"][0]["partial"], json!(true));
    }

    #[test]
    fn moonshot_moves_unclosed_think_prefill_into_reasoning_content() {
        let upstream = build_payload(json!({
            "model": "kimi-k3",
            "messages": [
                {"role": "assistant", "content": "<think>unfinished reasoning"}
            ]
        }));

        assert_eq!(
            upstream["messages"][0]["reasoning_content"],
            json!("unfinished reasoning")
        );
        assert_eq!(upstream["messages"][0]["content"], json!(""));
        assert_eq!(upstream["messages"][0]["partial"], json!(true));
    }

    #[test]
    fn moonshot_does_not_mark_assistant_prefill_partial_with_json_schema() {
        let upstream = build_payload(json!({
            "model": "kimi-k3",
            "json_schema": {"value": {"type": "object"}},
            "messages": [
                {"role": "assistant", "content": "prefill"}
            ]
        }));

        assert_eq!(upstream["messages"][0]["partial"], Value::Null);
    }

    #[test]
    fn moonshot_does_not_mark_assistant_prefill_partial_when_tools_are_present() {
        let upstream = build_payload(json!({
            "model": "kimi-k3",
            "tools": [{"type": "function", "function": {"name": "example"}}],
            "messages": [
                {"role": "assistant", "content": "prefill"}
            ]
        }));

        assert_eq!(upstream["messages"][0]["partial"], Value::Null);
    }

    #[test]
    fn moonshot_does_not_mark_assistant_prefill_partial_when_tool_message_exists() {
        let upstream = build_payload(json!({
            "model": "kimi-k3",
            "messages": [
                {"role": "assistant", "content": "earlier"},
                {"role": "tool", "content": "tool result"},
                {"role": "assistant", "content": "prefill"}
            ]
        }));

        assert_eq!(upstream["messages"][2]["partial"], Value::Null);
    }
}
