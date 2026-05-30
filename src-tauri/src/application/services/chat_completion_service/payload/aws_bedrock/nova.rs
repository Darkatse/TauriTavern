//! Amazon Nova on AWS Bedrock.
//!
//! Nova accepts the *same* Converse-style schema on `/model/{id}/invoke`
//! that the dedicated Converse API uses. The streaming variant returns
//! Converse-style EventStream frames (`messageStart`, `contentBlockDelta`,
//! `messageStop`, `metadata`); chunk normalization lives in the
//! infrastructure layer.
//!
//! Request body shape (per AWS Bedrock User Guide — `model-card-amazon-nova-*`,
//! `prompt-caching.md`, and the Converse API mapping):
//! ```json
//! {
//!   "system": [{ "text": "..." }],
//!   "messages": [
//!     { "role": "user", "content": [{ "text": "..." }] }
//!   ],
//!   "inferenceConfig": { "maxTokens": 300, "topP": 0.1, "topK": 20, "temperature": 0.3 }
//! }
//! ```
//!
//! Non-stream response:
//! ```json
//! { "output": { "message": { "role": "assistant", "content": [{ "text": "..." }] } },
//!   "stopReason": "end_turn", "usage": { "inputTokens": N, "outputTokens": M } }
//! ```
//!
//! Stream chunk (decoded base64 bytes of each EventStream frame):
//! ```json
//! { "contentBlockDelta": { "delta": { "text": "..." }, "contentBlockIndex": 0 } }
//! ```

use serde_json::{Map, Number, Value, json};

use super::shared::{
    BEDROCK_INVOKE_SUFFIX, FlatMessage, flatten_openai_messages, value_to_positive_i64,
};
use crate::application::errors::ApplicationError;

pub(super) fn build(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let (system_text, conversation) = flatten_openai_messages(payload.get("messages"));

    let nova_messages: Vec<Value> = conversation
        .into_iter()
        .map(|FlatMessage { role, text }| {
            let role = if role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            json!({
                "role": role,
                "content": [{ "text": text }],
            })
        })
        .collect();

    let mut body = Map::new();
    body.insert("messages".to_string(), Value::Array(nova_messages));

    if let Some(text) = system_text.filter(|value| !value.is_empty()) {
        body.insert(
            "system".to_string(),
            Value::Array(vec![json!({ "text": text })]),
        );
    }

    let inference_config = build_inference_config(&payload);
    if !inference_config.is_empty() {
        body.insert(
            "inferenceConfig".to_string(),
            Value::Object(inference_config),
        );
    }

    Ok((
        format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}"),
        Value::Object(body),
    ))
}

fn build_inference_config(payload: &Map<String, Value>) -> Map<String, Value> {
    let mut config = Map::new();

    if let Some(max_tokens) = value_to_positive_i64(payload.get("max_tokens")) {
        config.insert(
            "maxTokens".to_string(),
            Value::Number(Number::from(max_tokens)),
        );
    }
    if let Some(temperature) = payload.get("temperature").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(temperature) {
            config.insert("temperature".to_string(), Value::Number(number));
        }
    }
    if let Some(top_p) = payload.get("top_p").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(top_p) {
            config.insert("topP".to_string(), Value::Number(number));
        }
    }
    if let Some(top_k) = value_to_positive_i64(payload.get("top_k")) {
        config.insert("topK".to_string(), Value::Number(Number::from(top_k)));
    }
    if let Some(stop) = payload
        .get("stop")
        .cloned()
        .filter(|value| !value.is_null())
    {
        // Bedrock Converse-style payload accepts `stopSequences`.
        let stops = match stop {
            Value::Array(values) => values
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>(),
            Value::String(value) => vec![value],
            _ => Vec::new(),
        };
        if !stops.is_empty() {
            config.insert(
                "stopSequences".to_string(),
                Value::Array(stops.into_iter().map(Value::String).collect()),
            );
        }
    }

    config
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build;

    #[test]
    fn build_nova_emits_converse_style_invoke_body_for_inference_profile() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.amazon.nova-pro-v1:0",
            "messages": [
                { "role": "system", "content": "be concise" },
                { "role": "user", "content": "hello" }
            ],
            "max_tokens": 256,
            "temperature": 0.4,
            "top_p": 0.9,
            "top_k": 50,
            "stop": ["###"],
            "stream": true,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");

        assert_eq!(endpoint_path, "/model/us.amazon.nova-pro-v1:0/invoke");
        let body = body.as_object().expect("body should be object");

        // Body must not leak the routing-only fields.
        assert!(body.get("model").is_none());
        assert!(body.get("stream").is_none());

        let system = body
            .get("system")
            .and_then(Value::as_array)
            .expect("nova must lift system messages out of the conversation");
        assert_eq!(
            system[0].get("text").and_then(Value::as_str),
            Some("be concise"),
        );

        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages");
        assert_eq!(messages.len(), 1, "system was lifted out");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"][0]["text"], "hello");

        let inference = body
            .get("inferenceConfig")
            .and_then(Value::as_object)
            .expect("nova must always carry an inferenceConfig when params are present");
        assert_eq!(
            inference.get("maxTokens").and_then(Value::as_i64),
            Some(256)
        );
        assert_eq!(
            inference.get("temperature").and_then(Value::as_f64),
            Some(0.4)
        );
        assert_eq!(inference.get("topP").and_then(Value::as_f64), Some(0.9));
        assert_eq!(inference.get("topK").and_then(Value::as_i64), Some(50));
        let stop = inference
            .get("stopSequences")
            .and_then(Value::as_array)
            .expect("stopSequences");
        assert_eq!(stop[0], "###");
    }

    #[test]
    fn build_nova_falls_back_to_user_role_when_no_system_messages_present() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "amazon.nova-micro-v1:0",
            "messages": [{ "role": "user", "content": "hi" }],
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_, body) = build(payload).expect("payload should build");
        let body = body.as_object().expect("body should be object");
        assert!(
            body.get("system").is_none(),
            "system block should be omitted when no system messages exist",
        );
        assert_eq!(
            body.get("messages")
                .and_then(Value::as_array)
                .map(|messages| messages.len()),
            Some(1),
        );
    }
}
