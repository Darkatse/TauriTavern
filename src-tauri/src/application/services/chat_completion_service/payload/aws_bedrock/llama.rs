//! Meta Llama on AWS Bedrock.
//!
//! Llama 3 / 3.1 / 3.2 / 3.3 / 4 all share the same chat template format on
//! Bedrock's `/model/{id}/invoke` endpoint:
//!
//! ```text
//! <|begin_of_text|>
//! <|start_header_id|>system<|end_header_id|>
//!
//! {system}<|eot_id|>
//! <|start_header_id|>user<|end_header_id|>
//!
//! {user}<|eot_id|>
//! <|start_header_id|>assistant<|end_header_id|>
//!
//! ```
//!
//! Request body (per AWS Bedrock User Guide `model-parameters-meta.md`):
//! ```json
//! { "prompt": "<|begin_of_text|>...", "max_gen_len": 512, "temperature": 0.5, "top_p": 0.9 }
//! ```
//!
//! Non-stream response:
//! ```json
//! { "generation": "...", "prompt_token_count": N, "generation_token_count": M, "stop_reason": "stop" }
//! ```
//!
//! Stream chunk (one decoded EventStream frame per token group):
//! ```json
//! { "generation": "...", "prompt_token_count": ..., "generation_token_count": ..., "stop_reason": null|"stop" }
//! ```

use serde_json::{Map, Number, Value};

use super::shared::{
    BEDROCK_INVOKE_SUFFIX, FlatMessage, flatten_openai_messages, value_to_positive_i64,
};
use crate::application::errors::ApplicationError;

pub(super) fn build(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let (system_text, conversation) = flatten_openai_messages(payload.get("messages"));
    let prompt = format_llama3_chat_prompt(system_text.as_deref(), &conversation);

    let mut body = Map::new();
    body.insert("prompt".to_string(), Value::String(prompt));

    if let Some(max_tokens) = value_to_positive_i64(payload.get("max_tokens")) {
        body.insert(
            "max_gen_len".to_string(),
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

    Ok((
        format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}"),
        Value::Object(body),
    ))
}

/// Render a flat (system, [turns]) conversation as the canonical Llama 3 chat
/// template. The template is identical for 3.1, 3.2, 3.3, and 4 Instruct
/// models per `model-parameters-meta.md`.
pub(super) fn format_llama3_chat_prompt(system: Option<&str>, turns: &[FlatMessage]) -> String {
    let mut out = String::from("<|begin_of_text|>");

    if let Some(system) = system.map(str::trim).filter(|value| !value.is_empty()) {
        out.push_str("<|start_header_id|>system<|end_header_id|>\n\n");
        out.push_str(system);
        out.push_str("<|eot_id|>");
    }

    for turn in turns {
        let header = if turn.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        out.push_str("<|start_header_id|>");
        out.push_str(header);
        out.push_str("<|end_header_id|>\n\n");
        out.push_str(&turn.text);
        out.push_str("<|eot_id|>");
    }

    // Always prime the model for an assistant turn.
    out.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
    out
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build;
    use super::super::shared::FlatMessage;
    use super::format_llama3_chat_prompt;

    #[test]
    fn build_llama_emits_prompt_with_llama3_chat_template_and_max_gen_len() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.meta.llama3-3-70b-instruct-v1:0",
            "messages": [
                { "role": "system", "content": "be concise" },
                { "role": "user", "content": "hi" }
            ],
            "max_tokens": 512,
            "temperature": 0.4,
            "top_p": 0.9,
            "stream": true,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");
        assert_eq!(
            endpoint_path,
            "/model/us.meta.llama3-3-70b-instruct-v1:0/invoke",
        );

        let body = body.as_object().expect("body should be object");
        assert!(
            body.get("messages").is_none(),
            "llama uses prompt, not messages"
        );
        assert_eq!(
            body.get("max_gen_len").and_then(Value::as_i64),
            Some(512),
            "max_tokens must be renamed to Llama's max_gen_len",
        );
        assert_eq!(body.get("temperature").and_then(Value::as_f64), Some(0.4));
        assert_eq!(body.get("top_p").and_then(Value::as_f64), Some(0.9));
        assert!(body.get("stream").is_none());

        let prompt = body
            .get("prompt")
            .and_then(Value::as_str)
            .expect("prompt should be a string");
        assert!(prompt.starts_with("<|begin_of_text|>"));
        assert!(
            prompt.contains("<|start_header_id|>system<|end_header_id|>\n\nbe concise<|eot_id|>"),
            "system block must be present: {prompt}",
        );
        assert!(
            prompt.contains("<|start_header_id|>user<|end_header_id|>\n\nhi<|eot_id|>"),
            "user block must be present: {prompt}",
        );
        assert!(
            prompt.ends_with("<|start_header_id|>assistant<|end_header_id|>\n\n"),
            "must prime an assistant turn at the end: {prompt}",
        );
    }

    #[test]
    fn format_llama3_chat_prompt_handles_multi_turn_without_system_message() {
        let prompt = format_llama3_chat_prompt(
            None,
            &[
                FlatMessage {
                    role: "user".to_string(),
                    text: "hi".to_string(),
                },
                FlatMessage {
                    role: "assistant".to_string(),
                    text: "hello".to_string(),
                },
                FlatMessage {
                    role: "user".to_string(),
                    text: "again".to_string(),
                },
            ],
        );

        assert!(prompt.starts_with("<|begin_of_text|><|start_header_id|>user<|end_header_id|>"));
        assert!(
            prompt.contains("<|start_header_id|>assistant<|end_header_id|>\n\nhello<|eot_id|>")
        );
        assert!(prompt.contains("<|start_header_id|>user<|end_header_id|>\n\nagain<|eot_id|>"));
        assert!(prompt.ends_with("<|start_header_id|>assistant<|end_header_id|>\n\n"));
    }
}
