//! Mistral on AWS Bedrock.
//!
//! Bedrock hosts two distinct Mistral schemas that share neither request nor
//! response shapes, switched by model id:
//!
//! 1. **Text-completion (pre-2407 / Mixtral / 7B)** — `prompt`-based with the
//!    `<s>[INST] ... [/INST]` instruct template.
//!    Request: `{ "prompt": "...", "max_tokens": N, "temperature": ..., "top_p": ..., "top_k": ... }`
//!    Response: `{ "outputs": [{ "text": "...", "stop_reason": "..." }] }`
//!    Stream chunk: `{ "outputs": [{ "text": "...", "stop_reason": null|"stop" }] }`
//!
//! 2. **Chat-completion (mistral-large-2407+, Mistral Small / Medium / Pixtral)** —
//!    OpenAI-style `messages` + `tools`.
//!    Request: `{ "messages": [{"role":"user","content":"..."}], "max_tokens": ..., "temperature": ..., "top_p": ..., "tools": [...] }`
//!    Response (mistral-large-2407 doc): `{ "content": [{ "role": "assistant", "content": [{ "text": "..." }] }] }`
//!    Response (mistral-chat-completion doc): `{ "choices": [{ "index":0, "message":{"role":"assistant","content":"string"}, "stop_reason":"stop" }] }`
//!    Stream chunk (mistral-large-2407): `{ "content": [{ "text": "..." }] }`
//!    Stream chunk (chat-completion): `{ "choices": [{ "delta": { "content": "..." } }] }`
//!
//! Dispatch is driven by the Bedrock model id: anything matching `mistral-7b`,
//! `mixtral`, or the pre-2407 `-2402` cohort takes the legacy text-completion
//! path; everything else (2407+, large, small, medium, pixtral, ...) is
//! treated as chat.

use serde_json::{Map, Number, Value};

use super::shared::{
    BEDROCK_INVOKE_SUFFIX, FlatMessage, flatten_openai_messages, passthrough_chat_messages,
    value_to_positive_i64,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::bedrock_model::is_mistral_text_completion_model;

pub(super) fn build(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let endpoint_path = format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}");

    if is_legacy_text_completion(model_id) {
        return Ok((endpoint_path, build_text_completion_body(payload)));
    }

    Ok((endpoint_path, build_chat_completion_body(payload)))
}

pub(super) fn is_legacy_text_completion(model_id: &str) -> bool {
    is_mistral_text_completion_model(model_id)
}

fn build_text_completion_body(payload: Map<String, Value>) -> Value {
    let (system_text, conversation) = flatten_openai_messages(payload.get("messages"));
    let prompt = format_instruct_prompt(system_text.as_deref(), &conversation);

    let mut body = Map::new();
    body.insert("prompt".to_string(), Value::String(prompt));

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
    if let Some(top_k) = value_to_positive_i64(payload.get("top_k")) {
        body.insert("top_k".to_string(), Value::Number(Number::from(top_k)));
    }

    Value::Object(body)
}

fn build_chat_completion_body(payload: Map<String, Value>) -> Value {
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
    if let Some(tools) = payload
        .get("tools")
        .cloned()
        .filter(|value| value.is_array())
    {
        body.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = payload
        .get("tool_choice")
        .cloned()
        .filter(|value| !value.is_null())
    {
        body.insert("tool_choice".to_string(), tool_choice);
    }

    Value::Object(body)
}

/// Render a flat (system, [turns]) conversation as the `<s>[INST] ... [/INST]`
/// Mistral instruct template. The system text is prepended to the first user
/// message per Mistral's recommended prompt format.
pub(super) fn format_instruct_prompt(system: Option<&str>, turns: &[FlatMessage]) -> String {
    let mut prompt = String::from("<s>");
    let system_text = system
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let mut pending_system = system_text;
    let mut iter = turns.iter().peekable();
    while let Some(turn) = iter.next() {
        if turn.role == "assistant" {
            // Bare assistant turn without a preceding user turn — emit as
            // standalone completion text. Mistral handles this by just
            // appending the text after `[/INST]`.
            prompt.push(' ');
            prompt.push_str(&turn.text);
            prompt.push_str("</s>");
            continue;
        }

        prompt.push_str("[INST] ");
        if let Some(system_text) = pending_system.take() {
            prompt.push_str(&system_text);
            prompt.push_str("\n\n");
        }
        prompt.push_str(&turn.text);
        prompt.push_str(" [/INST]");

        if let Some(next) = iter.peek() {
            if next.role == "assistant" {
                let assistant = iter.next().expect("peek confirmed Some");
                prompt.push(' ');
                prompt.push_str(&assistant.text);
                prompt.push_str("</s>");
            }
        }
    }

    prompt
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build;

    #[test]
    fn build_mistral_legacy_7b_emits_instruct_prompt_with_max_tokens() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "mistral.mistral-7b-instruct-v0:2",
            "messages": [
                { "role": "system", "content": "be concise" },
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": "hello" },
                { "role": "user", "content": "again" }
            ],
            "max_tokens": 256,
            "temperature": 0.4,
            "top_p": 0.9,
            "top_k": 50,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");
        assert_eq!(
            endpoint_path,
            "/model/mistral.mistral-7b-instruct-v0:2/invoke"
        );
        let body = body.as_object().expect("body should be object");
        assert!(
            body.get("messages").is_none(),
            "legacy Mistral uses prompt-only, not messages",
        );
        let prompt = body
            .get("prompt")
            .and_then(Value::as_str)
            .expect("prompt must be a string");
        assert!(prompt.starts_with("<s>"), "must open with <s>: {prompt}");
        assert!(
            prompt.contains("[INST] be concise\n\nhi [/INST] hello</s>"),
            "system text must be prepended to first user turn: {prompt}",
        );
        assert!(
            prompt.ends_with("[INST] again [/INST]"),
            "trailing user turn must be primed for assistant completion: {prompt}",
        );
        assert_eq!(body.get("max_tokens").and_then(Value::as_i64), Some(256));
        assert_eq!(body.get("temperature").and_then(Value::as_f64), Some(0.4));
        assert_eq!(body.get("top_p").and_then(Value::as_f64), Some(0.9));
        assert_eq!(body.get("top_k").and_then(Value::as_i64), Some(50));
    }

    #[test]
    fn build_mistral_chat_2407_emits_openai_style_messages_body() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "mistral.mistral-large-2407-v1:0",
            "messages": [
                { "role": "system", "content": "you are a helpful assistant" },
                { "role": "user", "content": "hi" }
            ],
            "max_tokens": 1024,
            "temperature": 0.4,
            "top_p": 0.9,
            "tools": [{"type":"function","function":{"name":"foo","parameters":{}}}],
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");
        assert_eq!(
            endpoint_path,
            "/model/mistral.mistral-large-2407-v1:0/invoke"
        );
        let body = body.as_object().expect("body should be object");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages array");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "you are a helpful assistant");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "hi");
        assert!(
            body.get("prompt").is_none(),
            "chat path must not emit a prompt"
        );
        assert_eq!(body.get("max_tokens").and_then(Value::as_i64), Some(1024));
        assert_eq!(body.get("temperature").and_then(Value::as_f64), Some(0.4));
        assert!(body.get("tools").is_some(), "tools array passes through");
    }
}
