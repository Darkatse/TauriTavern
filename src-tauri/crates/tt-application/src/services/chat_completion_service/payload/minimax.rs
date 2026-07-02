use serde_json::{Map, Number, Value};

use super::prompt_post_processing::{PromptNames, PromptProcessingType, post_process_prompt};
use super::shared::insert_if_present;

const MINIMAX_ENDPOINT_PATH: &str = "/chat/completions";
const M2_HER_MAX_TOKENS: u64 = 2048;
const MINIMAX_REQUEST_FIELDS: &[&str] = &[
    "messages",
    "model",
    "temperature",
    "max_tokens",
    "stream",
    "top_p",
    "stop",
];

pub(super) fn build(payload: Map<String, Value>) -> (String, Value) {
    let mut payload = payload;
    merge_consecutive_tool_messages(&mut payload);

    let mut upstream_payload = build_request_payload(&payload);
    cap_m2_her_max_tokens(&mut upstream_payload);

    (
        MINIMAX_ENDPOINT_PATH.to_string(),
        Value::Object(upstream_payload),
    )
}

fn build_request_payload(payload: &Map<String, Value>) -> Map<String, Value> {
    let mut request = Map::new();

    for key in MINIMAX_REQUEST_FIELDS {
        insert_if_present(&mut request, payload, key);
    }

    if payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
    {
        insert_if_present(&mut request, payload, "tools");
        insert_if_present(&mut request, payload, "tool_choice");
    }

    request
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

fn cap_m2_her_max_tokens(payload: &mut Map<String, Value>) {
    let is_m2_her = payload
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|model| model == "M2-her");
    if !is_m2_her {
        return;
    }

    let Some(max_tokens) = payload.get_mut("max_tokens") else {
        return;
    };

    if let Some(value) = max_tokens.as_u64() {
        if value > M2_HER_MAX_TOKENS {
            *max_tokens = Value::Number(Number::from(M2_HER_MAX_TOKENS));
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn minimax_merges_consecutive_same_role_messages_and_preserves_tools() {
        let payload = json!({
            "chat_completion_source": "minimax",
            "model": "MiniMax-M2.7",
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

        let (endpoint_path, upstream) = build(payload);
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
    fn minimax_uses_provider_allowlist_and_fixed_chat_endpoint() {
        let payload = json!({
            "chat_completion_source": "minimax",
            "model": "MiniMax-M2.7",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7,
            "max_tokens": 1024,
            "stream": false,
            "top_p": 0.9,
            "stop": ["END"],
            "presence_penalty": 0.2,
            "frequency_penalty": 0.3,
            "top_k": 40,
            "logit_bias": {"1": -100},
            "n": 2,
            "user": "local",
            "response_format": {"type": "json_object"},
            "minimax_endpoint": "cn"
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, upstream) = build(payload);
        assert_eq!(endpoint_path, "/chat/completions");

        let body = upstream
            .as_object()
            .expect("upstream payload should be object");
        for key in [
            "presence_penalty",
            "frequency_penalty",
            "top_k",
            "logit_bias",
            "n",
            "user",
            "response_format",
            "minimax_endpoint",
            "chat_completion_source",
        ] {
            assert!(body.get(key).is_none(), "{key} must not be forwarded");
        }
        assert_eq!(body.get("temperature").and_then(Value::as_f64), Some(0.7));
        assert_eq!(body.get("top_p").and_then(Value::as_f64), Some(0.9));
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn minimax_omits_empty_tools() {
        let payload = json!({
            "chat_completion_source": "minimax",
            "model": "MiniMax-M2.7",
            "messages": [{"role": "user", "content": "hello"}],
            "tools": [],
            "tool_choice": "auto"
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_endpoint_path, upstream) = build(payload);
        assert!(upstream.get("tools").is_none());
        assert!(upstream.get("tool_choice").is_none());
    }

    #[test]
    fn minimax_caps_m2_her_max_tokens() {
        let payload = json!({
            "chat_completion_source": "minimax",
            "model": "M2-her",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 4096
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_endpoint_path, upstream) = build(payload);
        assert_eq!(upstream["max_tokens"].as_u64(), Some(2048));
    }
}
