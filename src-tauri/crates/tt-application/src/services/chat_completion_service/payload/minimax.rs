use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

use super::super::model_capabilities::{VersionedModel, map_minimax_reasoning_effort};
use super::prompt_post_processing::{PromptNames, PromptProcessingType, post_process_prompt};
use super::shared::insert_if_present;

const MINIMAX_ENDPOINT_PATH: &str = "/chat/completions";
const MINIMAX_REQUEST_FIELDS: &[&str] = &[
    "messages",
    "model",
    "temperature",
    "stream",
    "top_p",
    "stop",
];

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let mut payload = payload;
    merge_consecutive_tool_messages(&mut payload);

    Ok((
        MINIMAX_ENDPOINT_PATH.to_string(),
        Value::Object(build_request_payload(&payload)?),
    ))
}

fn build_request_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let mut request = Map::new();

    for key in MINIMAX_REQUEST_FIELDS {
        insert_if_present(&mut request, payload, key);
    }
    if let Some(max_tokens) = payload.get("max_tokens").filter(|value| !value.is_null()) {
        request.insert("max_completion_tokens".to_string(), max_tokens.clone());
    }

    if payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
    {
        insert_if_present(&mut request, payload, "tools");
        insert_if_present(&mut request, payload, "tool_choice");
    }

    let always_thinks = payload
        .get("model")
        .and_then(Value::as_str)
        .and_then(|model| VersionedModel::parse(model, "minimax-m"))
        .is_some_and(|minimax| minimax.version == (3, 1) && minimax.is("flash"));
    if !always_thinks && payload.get("include_reasoning").and_then(Value::as_bool) == Some(false) {
        request.insert("thinking".to_string(), json!({ "type": "disabled" }));
    }
    if let Some(reasoning_effort) = payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(map_minimax_reasoning_effort)
        .transpose()?
        .flatten()
    {
        request.insert(
            "reasoning_effort".to_string(),
            Value::String(reasoning_effort.to_string()),
        );
    }
    // Thinking never appears in `content`.
    request.insert("reasoning_split".to_string(), Value::Bool(true));

    Ok(request)
}

fn merge_consecutive_tool_messages(payload: &mut Map<String, Value>) {
    let names = PromptNames::from_payload(payload);

    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };

    let messages = std::mem::take(messages);
    payload.insert(
        "messages".to_string(),
        Value::Array(post_process_prompt(
            messages,
            PromptProcessingType::MergeTools,
            &names,
        )),
    );
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn minimax_merges_consecutive_same_role_messages_and_preserves_tools() {
        let payload = json!({
            "chat_completion_source": "minimax",
            "model": "MiniMax-M3",
            "messages": [
                {"role": "user", "content": "one"},
                {"role": "user", "content": "two"},
                {"role": "assistant", "content": "three"}
            ],
            "tools": [{"type": "function", "function": {"name": "search", "parameters": {}}}],
            "tool_choice": "auto"
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, upstream) = build(payload).expect("payload should build");
        assert_eq!(endpoint_path, "/chat/completions");

        let body = upstream
            .as_object()
            .expect("upstream payload should be object");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages should be array");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "one\n\ntwo");
        assert!(body.get("tools").is_some());
        assert_eq!(
            body.get("tool_choice").and_then(Value::as_str),
            Some("auto")
        );
    }

    #[test]
    fn minimax_sends_the_thinking_controls_each_model_accepts() {
        for (model, thinking) in [
            ("MiniMax-M3", Some("disabled")),
            ("MiniMax-M3.1-Flash", None),
        ] {
            let payload = json!({
                "chat_completion_source": "minimax",
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 4096,
                "include_reasoning": false,
                "reasoning_effort": "min"
            })
            .as_object()
            .cloned()
            .expect("payload should be object");

            let (_endpoint_path, upstream) = build(payload).expect("payload should build");

            assert_eq!(
                upstream.pointer("/thinking/type").and_then(Value::as_str),
                thinking,
                "{model}"
            );
            assert_eq!(upstream["reasoning_effort"], "low", "{model}");
            assert_eq!(upstream["reasoning_split"], true, "{model}");
            assert_eq!(upstream["max_completion_tokens"], 4096, "{model}");
            assert!(upstream.get("max_tokens").is_none(), "{model}");
        }
    }
}
