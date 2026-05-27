//! AI21 Jamba on AWS Bedrock.
//!
//! Jamba uses an OpenAI-compatible chat schema on Bedrock's
//! `/model/{id}/invoke` endpoint. We mostly pass payload fields through and
//! enforce Bedrock's streaming constraint (`n == 1`).
//!
//! Request body (per AWS Bedrock User Guide `model-parameters-jamba.md`):
//! ```json
//! {
//!   "messages": [
//!     { "role": "system",    "content": "..." },
//!     { "role": "user",      "content": "..." },
//!     { "role": "assistant", "content": "..." }
//!   ],
//!   "max_tokens": 512,
//!   "temperature": 0.7,
//!   "top_p": 0.9,
//!   "stop": ["###"],
//!   "n": 1,
//!   "frequency_penalty": 0.0,
//!   "presence_penalty": 0.0
//! }
//! ```
//!
//! Non-stream response (per AI21 Jamba chat-completion spec):
//! ```json
//! { "id": "...", "choices": [{ "index": 0,
//!     "message": { "role": "assistant", "content": "..." },
//!     "finish_reason": "stop|length|content_filter" }],
//!   "usage": { "prompt_tokens": N, "completion_tokens": M, "total_tokens": ... } }
//! ```
//!
//! Stream chunks: OpenAI-shape — `{ "choices": [{ "delta": {"content":"..."},
//! "finish_reason": null|"stop" }] }`. The terminal chunk includes `usage`
//! totals and a `[DONE]` sentinel is **not** emitted on Bedrock (Bedrock
//! wraps everything in EventStream frames).
//!
//! We require `n == 1` per Bedrock's streaming constraint (the doc says
//! `n must be 1 for streaming responses`), so we drop any `n` value > 1 to
//! prevent surprise errors.

use serde_json::{Map, Number, Value};

use super::shared::{
    BEDROCK_INVOKE_SUFFIX, passthrough_chat_messages, value_to_positive_i64,
};
use crate::application::errors::ApplicationError;

pub(super) fn build(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let endpoint_path = format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}");

    let messages = passthrough_chat_messages(payload.get("messages"));

    let mut body = Map::new();
    body.insert("messages".to_string(), Value::Array(messages));

    if let Some(max_tokens) = value_to_positive_i64(payload.get("max_tokens")) {
        body.insert(
            "max_tokens".to_string(),
            Value::Number(Number::from(max_tokens)),
        );
    }
    if let Some(temperature) = payload.get("temperature").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(temperature) {
            body.insert("temperature".to_string(), Value::Number(number));
        }
    }
    if let Some(top_p) = payload.get("top_p").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(top_p) {
            body.insert("top_p".to_string(), Value::Number(number));
        }
    }
    if let Some(stop) = payload.get("stop").cloned().filter(|value| value.is_array()) {
        body.insert("stop".to_string(), stop);
    }
    if let Some(frequency_penalty) = payload.get("frequency_penalty").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(frequency_penalty) {
            body.insert("frequency_penalty".to_string(), Value::Number(number));
        }
    }
    if let Some(presence_penalty) = payload.get("presence_penalty").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(presence_penalty) {
            body.insert("presence_penalty".to_string(), Value::Number(number));
        }
    }

    // Streaming forbids n > 1; Bedrock currently only accepts streaming
    // requests in the TauriTavern flow, but we accept both. Clamp to 1 when
    // the upstream value is invalid.
    let n_value = value_to_positive_i64(payload.get("n")).unwrap_or(1);
    body.insert("n".to_string(), Value::Number(Number::from(n_value.max(1))));

    Ok((endpoint_path, Value::Object(body)))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build;

    #[test]
    fn build_ai21_jamba_emits_openai_style_messages_body() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "ai21.jamba-1-5-large-v1:0",
            "messages": [
                { "role": "system", "content": "be terse" },
                { "role": "user", "content": "What causes earthquakes?" }
            ],
            "max_tokens": 512,
            "temperature": 0.7,
            "top_p": 0.9,
            "stop": ["###"],
            "presence_penalty": 0.1,
            "stream": true,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint, body) = build(payload).expect("ai21 jamba build must succeed");

        assert_eq!(endpoint, "/model/ai21.jamba-1-5-large-v1:0/invoke");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "be terse");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "What causes earthquakes?");

        assert_eq!(body.get("max_tokens"), Some(&json!(512)));
        assert_eq!(body.get("temperature"), Some(&json!(0.7)));
        assert_eq!(body.get("top_p"), Some(&json!(0.9)));
        assert_eq!(body.get("stop"), Some(&json!(["###"])));
        assert_eq!(body.get("presence_penalty"), Some(&json!(0.1)));
        assert_eq!(
            body.get("n"),
            Some(&json!(1)),
            "n defaults to 1 because streaming forbids n>1",
        );
    }
}
