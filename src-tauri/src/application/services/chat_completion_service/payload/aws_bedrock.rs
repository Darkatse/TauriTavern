//! AWS Bedrock chat-completion payload dispatcher.
//!
//! Each Bedrock model family (Anthropic Claude, Amazon Nova, Meta Llama,
//! Mistral, DeepSeek, Cohere Command R, AI21 Jamba, ...) has its own request
//! body schema, response shape and stream chunk encoding. This module owns:
//!
//! - Dispatch from the OpenAI-shape payload that arrives at the router into
//!   the provider-specific builder
//! - Shared helpers used across providers (flatten/passthrough message
//!   adapters, scalar coercion, the `invoke` URL suffix)
//!
//! Provider-specific request shaping lives in the matching submodule
//! (`anthropic.rs`, `nova.rs`, `llama.rs`, `mistral.rs`, `deepseek.rs`,
//! `cohere.rs`, `ai21_jamba.rs`). Provider-specific response/stream chunk
//! shaping lives in the symmetrical infrastructure module
//! (`infrastructure::apis::http_chat_completion_repository::aws_bedrock`).

use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::models::bedrock_model::{BedrockModelFamily, BedrockModelSpec};

mod ai21_jamba;
mod anthropic;
mod cohere;
mod custom;
mod deepseek;
mod llama;
mod mistral;
mod nova;
mod shared;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let model_id = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "AWS Bedrock requires a model id (e.g. anthropic.claude-sonnet-4-20250514-v1:0, us.amazon.nova-pro-v1:0, meta.llama3-70b-instruct-v1:0)"
                    .to_string(),
            )
        })?;

    // Custom-template escape hatch: when explicitly enabled by the user,
    // bypass automatic provider dispatch entirely. This lets users wire
    // Bedrock-hosted models we don't have a first-class builder for yet
    // (Titan Text, Writer, Stability, future variants) without waiting on a
    // backend change.
    if custom::is_enabled(&payload) {
        return custom::build(payload, &model_id);
    }

    let spec = BedrockModelSpec::classify(&model_id);
    match spec.family() {
        BedrockModelFamily::AnthropicClaude => anthropic::build(payload, &model_id),
        BedrockModelFamily::AmazonNova => nova::build(payload, &model_id),
        BedrockModelFamily::MetaLlama => llama::build(payload, &model_id),
        BedrockModelFamily::MistralTextCompletion | BedrockModelFamily::MistralChat => {
            mistral::build(payload, &model_id)
        }
        BedrockModelFamily::DeepSeekTextCompletion | BedrockModelFamily::DeepSeekChat => {
            deepseek::build(payload, &model_id)
        }
        BedrockModelFamily::CohereCommandR => cohere::build(payload, &model_id),
        BedrockModelFamily::Ai21Jamba => ai21_jamba::build(payload, &model_id),
        BedrockModelFamily::Unsupported => Err(ApplicationError::ValidationError(
            unsupported_model_message(&spec),
        )),
    }
}

fn unsupported_model_message(spec: &BedrockModelSpec) -> String {
    let reason = spec
        .unsupported_reason()
        .unwrap_or("This Bedrock model family is not wired by TauriTavern's built-in adapter yet.");
    format!(
        "AWS Bedrock model `{}` is not supported by TauriTavern's built-in Bedrock adapter. {reason} Enable the custom template (`aws_bedrock_use_custom_template`) for this model, or choose one of the supported families: Anthropic Claude, Amazon Nova, Meta Llama, Mistral, DeepSeek, Cohere Command R/R+, AI21 Jamba.",
        spec.raw_id()
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build;

    #[test]
    fn bedrock_requires_model_id() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let err = build(payload).expect_err("missing model should fail");
        assert!(
            err.to_string().contains("AWS Bedrock requires a model id"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn build_returns_clear_error_when_model_family_is_not_supported() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "amazon.titan-text-premier-v1:0",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let err = build(payload).expect_err("non-wired family must fail with a clear error");
        let message = err.to_string();
        assert!(
            message.contains("not supported"),
            "unexpected error: {message}",
        );
        assert!(
            message.contains("Titan"),
            "error must name the unsupported family: {message}",
        );
        assert!(
            message.contains("custom template"),
            "error must point users at the custom-template escape hatch: {message}",
        );
        assert!(
            message.contains("Anthropic Claude"),
            "error must mention currently supported providers: {message}",
        );
    }
}
