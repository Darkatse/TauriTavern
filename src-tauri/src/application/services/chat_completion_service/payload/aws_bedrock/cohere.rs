//! Cohere Command R / R+ on AWS Bedrock.
//!
//! Bedrock hosts Cohere's chat API on `/model/{id}/invoke`; the schema is
//! **not** OpenAI-shaped — the latest user message is split out into a
//! dedicated `message` field while prior turns are projected into a
//! `chat_history` array of `{role, message}` entries.
//!
//! Request body (per AWS Bedrock User Guide
//! `model-parameters-cohere-command-r-plus.md`):
//! ```json
//! {
//!   "message": "latest user turn",
//!   "chat_history": [
//!     { "role": "USER",    "message": "..." },
//!     { "role": "CHATBOT", "message": "..." }
//!   ],
//!   "preamble": "system prompt",
//!   "max_tokens": 512,
//!   "temperature": 0.5,
//!   "p": 0.5,        // top_p
//!   "k": 250,        // top_k
//!   "stop_sequences": ["..."]
//! }
//! ```
//!
//! Non-stream response:
//! ```json
//! { "text": "...", "finish_reason": "complete|max_tokens|error|...",
//!   "meta": { "billed_units": { "input_tokens": N, "output_tokens": M } } }
//! ```
//!
//! Stream chunks (each decoded EventStream frame):
//! - `{ "event_type": "stream-start", "generation_id": "..." }` — drop
//! - `{ "event_type": "text-generation", "text": "...", "is_finished": false }` — emit
//! - `{ "event_type": "stream-end", "finish_reason": "...", "response": {...} }` — drop
//! - `{ "event_type": "citation-generation" | "tool-calls-*" | ... }` — drop
//!
//! We currently route every `cohere.*` model through this builder (the modern
//! Command R API). Legacy `cohere.command-text-v14` /
//! `command-light-text-v14` expose a different `prompt`-based schema with
//! `generations[].text`; they will return a Bedrock validation error today
//! and are tracked for a follow-up. The frontend already gates `embed*` /
//! `rerank*` out of the chat-completion picker.

use serde_json::{Map, Number, Value, json};

use super::shared::{
    BEDROCK_INVOKE_SUFFIX, FlatMessage, flatten_openai_messages, value_to_positive_i64,
};
use crate::application::errors::ApplicationError;

pub(super) fn build(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let endpoint_path = format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}");

    let (system_text, mut conversation) = flatten_openai_messages(payload.get("messages"));

    // Cohere's chat API requires the latest user turn in `message`. If the
    // conversation ends with an assistant turn (rare — usually a re-roll path)
    // we still send an empty `message` so Bedrock doesn't reject the call.
    let last_user = conversation
        .iter()
        .rposition(|turn| turn.role == "user")
        .map(|index| conversation.remove(index));

    let chat_history = build_chat_history(&conversation);
    let message = last_user.map(|turn| turn.text).unwrap_or_default();

    let mut body = Map::new();
    body.insert("message".to_string(), Value::String(message));
    if !chat_history.is_empty() {
        body.insert("chat_history".to_string(), Value::Array(chat_history));
    }
    if let Some(text) = system_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert("preamble".to_string(), Value::String(text.to_string()));
    }

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
    // Cohere uses `p` / `k` instead of `top_p` / `top_k`.
    if let Some(top_p) = payload.get("top_p").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(top_p) {
            body.insert("p".to_string(), Value::Number(number));
        }
    }
    if let Some(top_k) = value_to_positive_i64(payload.get("top_k")) {
        body.insert("k".to_string(), Value::Number(Number::from(top_k)));
    }
    if let Some(seed) = value_to_positive_i64(payload.get("seed")) {
        body.insert("seed".to_string(), Value::Number(Number::from(seed)));
    }
    if let Some(stop) = payload.get("stop").cloned().filter(|value| value.is_array()) {
        body.insert("stop_sequences".to_string(), stop);
    } else if let Some(stop) = payload
        .get("stop_sequences")
        .cloned()
        .filter(|value| value.is_array())
    {
        body.insert("stop_sequences".to_string(), stop);
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

    Ok((endpoint_path, Value::Object(body)))
}

/// Project the leading (non-latest-user) turns into Cohere's
/// `chat_history: [{role:"USER"|"CHATBOT", message: string}]` array. Empty
/// messages are skipped so we don't poison Cohere's context.
pub(super) fn build_chat_history(turns: &[FlatMessage]) -> Vec<Value> {
    turns
        .iter()
        .filter_map(|turn| {
            if turn.text.trim().is_empty() {
                return None;
            }
            let role = if turn.role == "assistant" {
                "CHATBOT"
            } else {
                "USER"
            };
            Some(json!({ "role": role, "message": turn.text }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build;
    use super::super::shared::FlatMessage;
    use super::build_chat_history;

    #[test]
    fn build_cohere_command_r_extracts_message_history_and_preamble() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "cohere.command-r-plus-v1:0",
            "messages": [
                { "role": "system", "content": "be playful" },
                { "role": "user", "content": "Who discovered gravity?" },
                { "role": "assistant", "content": "Isaac Newton." },
                { "role": "user", "content": "When was he born?" }
            ],
            "max_tokens": 600,
            "temperature": 0.6,
            "top_p": 0.5,
            "top_k": 250,
            "stop": ["###"],
            "frequency_penalty": 0.2,
            "stream": true,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint, body) = build(payload).expect("cohere command-r build must succeed");

        assert_eq!(endpoint, "/model/cohere.command-r-plus-v1:0/invoke");
        assert_eq!(body.get("message"), Some(&json!("When was he born?")));
        let history = body
            .get("chat_history")
            .and_then(Value::as_array)
            .expect("chat_history must be present");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["role"], "USER");
        assert_eq!(history[0]["message"], "Who discovered gravity?");
        assert_eq!(history[1]["role"], "CHATBOT");
        assert_eq!(history[1]["message"], "Isaac Newton.");

        assert_eq!(body.get("preamble"), Some(&json!("be playful")));
        assert_eq!(body.get("max_tokens"), Some(&json!(600)));
        assert_eq!(body.get("temperature"), Some(&json!(0.6)));
        // p/k are Cohere's top_p/top_k aliases — must be remapped.
        assert_eq!(body.get("p"), Some(&json!(0.5)));
        assert_eq!(body.get("k"), Some(&json!(250)));
        assert_eq!(body.get("top_p"), None);
        assert_eq!(body.get("top_k"), None);
        assert_eq!(body.get("stop_sequences"), Some(&json!(["###"])));
        assert_eq!(body.get("frequency_penalty"), Some(&json!(0.2)));
    }

    #[test]
    fn build_chat_history_drops_empty_messages_and_marks_roles() {
        let turns = vec![
            FlatMessage {
                role: "user".to_string(),
                text: "hello".to_string(),
            },
            FlatMessage {
                role: "assistant".to_string(),
                text: "   ".to_string(),
            },
            FlatMessage {
                role: "assistant".to_string(),
                text: "hi back".to_string(),
            },
        ];
        let history = build_chat_history(&turns);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["role"], "USER");
        assert_eq!(history[0]["message"], "hello");
        assert_eq!(history[1]["role"], "CHATBOT");
        assert_eq!(history[1]["message"], "hi back");
    }
}
