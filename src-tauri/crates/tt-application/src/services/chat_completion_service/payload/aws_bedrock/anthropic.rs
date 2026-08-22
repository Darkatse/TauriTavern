//! Anthropic Messages on AWS Bedrock.
//!
//! Bedrock hosts every Claude generation on `/model/{id}/invoke` using the
//! standard Anthropic Messages schema, except that:
//!
//! - The body must omit `model` (the id lives in the URL path) and `stream`
//!   (Bedrock infers streaming from `/invoke-with-response-stream`).
//! - The body must include `anthropic_version: "bedrock-2023-05-31"`.
//! - Bedrock model ids carry an inference-profile prefix (`us.` / `eu.` /
//!   `jp.` / `au.` / `apac.` / `global.` / `us-gov.`), a provider segment (`anthropic.`)
//!   and an optional version suffix (`-v1`, `:0`). The Anthropic-direct
//!   [`crate::services::chat_completion_service::payload::claude::contract::ClaudeModelContract`]
//!   resolver expects the bare form (`claude-opus-4-7`,
//!   `claude-sonnet-4-5-20250929`, ...) so we normalize the id before
//!   delegating to the Claude builder.

use serde_json::{Map, Value};

use super::super::claude;
use super::shared::BEDROCK_INVOKE_SUFFIX;
use crate::errors::ApplicationError;
use tt_domain::models::bedrock_model::strip_inference_profile_prefix;

const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";
const BEDROCK_ANTHROPIC_PREFIX: &str = "anthropic.";

/// Build an Anthropic Messages payload by delegating to [`claude::build`] and
/// rewriting the result for Bedrock's `/model/{modelId}/invoke` endpoint.
pub(super) fn build(
    mut payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    // Bedrock model IDs (e.g. `us.anthropic.claude-opus-4-7`,
    // `anthropic.claude-opus-4-6-v1`, `anthropic.claude-sonnet-4-5-20250929-v1:0`)
    // do NOT match the Anthropic-direct prefixes (`claude-opus-4-7`...) that the
    // Claude payload builder uses to resolve thinking / sampling / output-effort
    // capabilities. Normalize before delegating so model contract resolution works.
    let normalized_model = normalize_bedrock_model_id(model_id);
    payload.insert("model".to_string(), Value::String(normalized_model));

    let (_, request) = claude::build(payload)?;

    let mut request_object = match request {
        Value::Object(map) => map,
        _ => {
            return Err(ApplicationError::InternalError(
                "Claude payload builder returned a non-object request".to_string(),
            ));
        }
    };

    reject_non_base64_image_sources(&request_object)?;

    request_object.remove("model");
    // Bedrock infers streaming from the URL path, not from a body field.
    request_object.remove("stream");

    request_object.insert(
        "anthropic_version".to_string(),
        Value::String(BEDROCK_ANTHROPIC_VERSION.to_string()),
    );

    // The endpoint path always carries the *original* Bedrock model id
    // (with inference-profile + provider prefix + version suffix intact).
    let endpoint_path = format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}");

    Ok((endpoint_path, Value::Object(request_object)))
}

fn reject_non_base64_image_sources(request: &Map<String, Value>) -> Result<(), ApplicationError> {
    let Some(messages) = request.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };

    for message in messages {
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };

        for block in content {
            if !is_claude_image_block(block) {
                continue;
            }

            let source_type = block
                .get("source")
                .and_then(Value::as_object)
                .and_then(|source| source.get("type"))
                .and_then(Value::as_str)
                .map(str::trim);

            if source_type != Some("base64") {
                return Err(ApplicationError::ValidationError(
                    "AWS Bedrock Claude only supports base64 image sources; send a data URL instead of a remote URL or provider file reference."
                        .to_string(),
                ));
            }
        }
    }

    Ok(())
}

fn is_claude_image_block(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|entry| entry.trim() == "image")
}

/// Convert a Bedrock model id into the Anthropic-direct form that
/// `payload::claude::contract::ClaudeModelContract::resolve` understands.
///
/// Examples:
/// - `us.anthropic.claude-opus-4-7`              -> `claude-opus-4-7`
/// - `global.anthropic.claude-opus-4-6-v1`       -> `claude-opus-4-6`
/// - `anthropic.claude-sonnet-4-5-20250929-v1:0` -> `claude-sonnet-4-5-20250929`
pub(super) fn normalize_bedrock_model_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut id = strip_inference_profile_prefix(trimmed);
    if let Some(rest) = id.strip_prefix(BEDROCK_ANTHROPIC_PREFIX) {
        id = rest;
    }
    // Bedrock version suffix can be `-v1:0`, `:0` (rare), or `-v1`.
    if let Some(rest) = id.strip_suffix(":0") {
        id = rest;
    }
    if let Some(rest) = id.strip_suffix("-v1") {
        id = rest;
    }
    id.to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::build;

    #[test]
    fn bedrock_moves_model_to_url_path_and_injects_anthropic_version() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "anthropic.claude-sonnet-4-20250514-v1:0",
            "messages": [{ "role": "user", "content": "hello" }],
            "stream": true,
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");

        assert_eq!(
            endpoint_path,
            "/model/anthropic.claude-sonnet-4-20250514-v1:0/invoke"
        );

        let body = body.as_object().expect("body should be object");
        assert!(
            body.get("model").is_none(),
            "model must be removed from body"
        );
        assert!(
            body.get("stream").is_none(),
            "stream must be removed; Bedrock infers it from the URL path",
        );
        assert_eq!(
            body.get("anthropic_version").and_then(Value::as_str),
            Some("bedrock-2023-05-31"),
        );
        assert_eq!(body.get("max_tokens").and_then(Value::as_u64), Some(1024));
    }

    #[test]
    fn bedrock_preserves_us_inference_profile_prefix_in_path() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 256,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, _) = build(payload).expect("payload should build");
        assert_eq!(
            endpoint_path,
            "/model/us.anthropic.claude-sonnet-4-5-20250929-v1:0/invoke"
        );
    }

    #[test]
    fn bedrock_claude_allows_base64_image_sources() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "anthropic.claude-sonnet-4-20250514-v1:0",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": { "url": "data:image/png;base64,AAAA" }
                }]
            }],
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_, body) = build(payload).expect("payload should build");
        assert_eq!(
            body.pointer("/messages/0/content/0/source/type")
                .and_then(Value::as_str),
            Some("base64")
        );
    }

    #[test]
    fn bedrock_claude_rejects_non_base64_image_sources() {
        for content in [
            json!([{
                "type": "image_url",
                "image_url": { "url": "https://example.test/cat.png" }
            }]),
            json!([{
                "type": "image",
                "source": {
                    "type": "url",
                    "url": "https://example.test/native.png"
                }
            }]),
            json!([{
                "type": "image",
                "source": {
                    "type": "file",
                    "file_id": "file_123"
                }
            }]),
        ] {
            let payload = json!({
                "chat_completion_source": "aws_bedrock",
                "model": "anthropic.claude-sonnet-4-20250514-v1:0",
                "messages": [{
                    "role": "user",
                    "content": content
                }],
                "max_tokens": 1024,
            })
            .as_object()
            .cloned()
            .expect("payload should be object");

            let error = build(payload).expect_err("non-base64 image source should fail");
            assert!(
                error
                    .to_string()
                    .contains("AWS Bedrock Claude only supports base64 image sources"),
                "{error}"
            );
        }
    }

    #[test]
    fn bedrock_unlocks_fable_5_adaptive_thinking_via_normalization() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.anthropic.claude-fable-5",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 4096,
            "reasoning_effort": "xhigh",
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");
        assert_eq!(
            endpoint_path, "/model/us.anthropic.claude-fable-5/invoke",
            "URL path must retain the raw Bedrock id"
        );
        assert_eq!(
            body.pointer("/thinking/type").and_then(Value::as_str),
            Some("adaptive")
        );
        assert_eq!(
            body.pointer("/output_config/effort")
                .and_then(Value::as_str),
            Some("xhigh")
        );
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("top_k").is_none());
    }
}
