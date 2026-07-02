//! DeepSeek on AWS Bedrock.
//!
//! Bedrock hosts two DeepSeek schemas, switched by model id:
//!
//! 1. **Text-completion (DeepSeek-R1)** — `prompt`-based with the R1
//!    instruction template
//!    `<｜begin▁of▁sentence｜><｜User｜>{user}<｜Assistant｜><think>\n`.
//!    Request: `{ "prompt": "...", "max_tokens": N, "temperature": ..., "top_p": ..., "stop": [...] }`
//!    Response: `{ "choices": [{ "text": "...", "stop_reason": "stop"|"length" }] }`
//!    Stream chunk (per EventStream frame): `{ "choices": [{ "text": "...", "stop_reason": null|"stop" }] }`.
//!
//! 2. **Chat-completion (DeepSeek-V3.1 / V3.2)** — OpenAI-style `messages`.
//!    Request: `{ "messages": [{"role","content"}], "max_tokens": N, "temperature": ..., "top_p": ... }`
//!    The Bedrock model card examples (V3.1, V3.2) only show the request and
//!    use `json.loads(response['body'].read())` to inspect the response — they
//!    do not pin the shape. The infrastructure layer's
//!    `deepseek_response_to_claude_shape` probes both `choices[].text` and
//!    `choices[].message.content` defensively.
//!
//! Dispatch is driven by the Bedrock model id: anything containing `r1` is
//! the reasoning model and takes the text-completion path with the R1
//! template; everything else (`v3`, `v3-1`, `v3.2`, ...) uses the
//! chat-completion path.

use serde_json::{Map, Number, Value};

use super::shared::{
    BEDROCK_INVOKE_SUFFIX, FlatMessage, flatten_openai_messages, passthrough_chat_messages,
    value_to_positive_i64,
};
use crate::errors::ApplicationError;
use tt_domain::models::bedrock_model::is_deepseek_text_completion_model;

pub(super) fn build(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let endpoint_path = format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}");

    if is_text_completion(model_id) {
        return Ok((endpoint_path, build_text_completion_body(payload)));
    }

    Ok((endpoint_path, build_chat_completion_body(payload)))
}

pub(super) fn is_text_completion(model_id: &str) -> bool {
    is_deepseek_text_completion_model(model_id)
}

fn build_text_completion_body(payload: Map<String, Value>) -> Value {
    let (system_text, conversation) = flatten_openai_messages(payload.get("messages"));
    let prompt = format_r1_prompt(system_text.as_deref(), &conversation);

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
    if let Some(stop) = payload
        .get("stop")
        .cloned()
        .filter(|value| value.is_array())
    {
        body.insert("stop".to_string(), stop);
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

    Value::Object(body)
}

/// Render a flat (system, [turns]) conversation in the DeepSeek-R1 instruction
/// template per the Bedrock `model-parameters-deepseek.md` example:
///
/// ```text
/// <｜begin▁of▁sentence｜>{system}<｜User｜>{user}<｜Assistant｜><think>\n
/// ```
///
/// Multi-turn history is rendered by alternating
/// `<｜User｜>...<｜Assistant｜>...<｜end▁of▁sentence｜>` blocks and ending the
/// prompt with `<｜Assistant｜><think>\n` so R1 starts its reasoning trace
/// immediately. Unicode characters (`｜` `▁`) match the model card verbatim.
pub(super) fn format_r1_prompt(system: Option<&str>, turns: &[FlatMessage]) -> String {
    let mut prompt = String::from("<｜begin▁of▁sentence｜>");
    if let Some(text) = system.map(str::trim).filter(|value| !value.is_empty()) {
        prompt.push_str(text);
    }

    let mut iter = turns.iter().peekable();
    while let Some(turn) = iter.next() {
        if turn.role == "assistant" {
            prompt.push_str("<｜Assistant｜>");
            prompt.push_str(&turn.text);
            prompt.push_str("<｜end▁of▁sentence｜>");
            continue;
        }

        prompt.push_str("<｜User｜>");
        prompt.push_str(&turn.text);

        if let Some(next) = iter.peek() {
            if next.role == "assistant" {
                let assistant = iter.next().expect("peek confirmed Some");
                prompt.push_str("<｜Assistant｜>");
                prompt.push_str(&assistant.text);
                prompt.push_str("<｜end▁of▁sentence｜>");
            }
        }
    }

    prompt.push_str("<｜Assistant｜><think>\n");
    prompt
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build;
    use super::super::shared::FlatMessage;
    use super::{format_r1_prompt, is_text_completion};

    #[test]
    fn is_text_completion_only_matches_r1_variants() {
        assert!(is_text_completion("deepseek.r1-v1:0"));
        assert!(is_text_completion("us.deepseek.r1-v1:0"));
        assert!(!is_text_completion("deepseek.v3-v1:0"));
        assert!(!is_text_completion("deepseek.v3.2"));
    }

    #[test]
    fn build_deepseek_r1_emits_text_completion_with_reasoning_template() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.deepseek.r1-v1:0",
            "messages": [
                { "role": "system", "content": "answer concisely" },
                { "role": "user", "content": "hi" }
            ],
            "max_tokens": 256,
            "temperature": 0.4,
            "top_p": 0.95,
            "stop": ["###"],
            "stream": true,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint, body) = build(payload).expect("deepseek r1 build must succeed");

        assert_eq!(endpoint, "/model/us.deepseek.r1-v1:0/invoke");
        assert_eq!(
            body.get("messages"),
            None,
            "text-completion path must not surface OpenAI messages array",
        );
        let prompt = body
            .get("prompt")
            .and_then(Value::as_str)
            .expect("prompt must be populated");
        assert!(prompt.starts_with("<｜begin▁of▁sentence｜>answer concisely<｜User｜>hi"));
        assert!(prompt.ends_with("<｜Assistant｜><think>\n"));

        assert_eq!(body.get("max_tokens"), Some(&json!(256)));
        assert_eq!(body.get("temperature"), Some(&json!(0.4)));
        assert_eq!(body.get("top_p"), Some(&json!(0.95)));
        assert_eq!(body.get("stop"), Some(&json!(["###"])));
    }

    #[test]
    fn build_deepseek_v3_chat_emits_openai_style_messages_body() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "deepseek.v3-v1:0",
            "messages": [
                { "role": "user", "content": "ping" }
            ],
            "max_tokens": 128,
            "temperature": 0.2,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint, body) = build(payload).expect("deepseek v3 build must succeed");

        assert_eq!(endpoint, "/model/deepseek.v3-v1:0/invoke");
        assert_eq!(body.get("prompt"), None);
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "ping");
        assert_eq!(body.get("max_tokens"), Some(&json!(128)));
        assert_eq!(body.get("temperature"), Some(&json!(0.2)));
    }

    #[test]
    fn format_r1_prompt_handles_multi_turn_history() {
        let turns = vec![
            FlatMessage {
                role: "user".to_string(),
                text: "Who is Newton?".to_string(),
            },
            FlatMessage {
                role: "assistant".to_string(),
                text: "Isaac Newton.".to_string(),
            },
            FlatMessage {
                role: "user".to_string(),
                text: "When was he born?".to_string(),
            },
        ];

        let prompt = format_r1_prompt(Some("be terse"), &turns);
        assert!(prompt.starts_with("<｜begin▁of▁sentence｜>be terse<｜User｜>Who is Newton?"));
        assert!(prompt.contains("<｜Assistant｜>Isaac Newton.<｜end▁of▁sentence｜>"));
        assert!(prompt.contains("<｜User｜>When was he born?"));
        assert!(prompt.ends_with("<｜Assistant｜><think>\n"));
    }
}
