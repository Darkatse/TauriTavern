//! Anthropic Messages on AWS Bedrock.
//!
//! Bedrock hosts every Claude generation on `/model/{id}/invoke` using the
//! standard Anthropic Messages schema, except that:
//!
//! - The body must omit `model` (the id lives in the URL path) and `stream`
//!   (Bedrock infers streaming from `/invoke-with-response-stream`).
//! - The body must include `anthropic_version: "bedrock-2023-05-31"`.
//! - Bedrock model ids carry an inference-profile prefix (`us.` / `eu.` /
//!   `apac.` / `global.` / `us-gov.`), a provider segment (`anthropic.`)
//!   and an optional version suffix (`-v1`, `:0`). The Anthropic-direct
//!   [`crate::application::services::chat_completion_service::payload::claude::contract::ClaudeModelContract`]
//!   resolver expects the bare form (`claude-opus-4-7`,
//!   `claude-sonnet-4-5-20250929`, ...) so we normalize the id before
//!   delegating to the Claude builder.

use serde_json::{Map, Value};

use super::super::claude;
use super::shared::BEDROCK_INVOKE_SUFFIX;
use crate::application::errors::ApplicationError;
use crate::domain::models::bedrock_model::strip_inference_profile_prefix;

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
    use super::normalize_bedrock_model_id;

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
    fn normalize_strips_inference_profile_provider_and_version_suffixes() {
        // Bare Bedrock catalog ids.
        assert_eq!(
            normalize_bedrock_model_id("anthropic.claude-opus-4-7"),
            "claude-opus-4-7"
        );
        assert_eq!(
            normalize_bedrock_model_id("anthropic.claude-opus-4-6-v1"),
            "claude-opus-4-6"
        );
        assert_eq!(
            normalize_bedrock_model_id("anthropic.claude-sonnet-4-5-20250929-v1:0"),
            "claude-sonnet-4-5-20250929"
        );
        // Cross-region inference profile ids.
        assert_eq!(
            normalize_bedrock_model_id("us.anthropic.claude-opus-4-7"),
            "claude-opus-4-7"
        );
        assert_eq!(
            normalize_bedrock_model_id("global.anthropic.claude-opus-4-6-v1"),
            "claude-opus-4-6"
        );
        // Already-normalized ids pass through unchanged.
        assert_eq!(
            normalize_bedrock_model_id("claude-3-5-sonnet-20240620"),
            "claude-3-5-sonnet-20240620"
        );
        // Padding tolerance.
        assert_eq!(
            normalize_bedrock_model_id("  us.anthropic.claude-opus-4-7  "),
            "claude-opus-4-7"
        );
        // Non-Anthropic ids: only the inference-profile prefix and the
        // version suffix are stripped — the provider segment stays. The
        // normalizer is only ever called on the Anthropic dispatch path, so
        // this is mostly defensive: we want a stable result regardless.
        assert_eq!(
            normalize_bedrock_model_id("us.amazon.nova-pro-v1:0"),
            "amazon.nova-pro",
            "us./global. + :0 + -v1 are stripped, provider `amazon.` is kept",
        );
    }

    #[test]
    fn bedrock_unlocks_opus_4_7_adaptive_thinking_via_normalization() {
        // Without normalization, `ClaudeModelContract::resolve` cannot match
        // `claude-opus-4-7` against `us.anthropic.claude-opus-4-7`, so the
        // builder would silently strip `reasoning_effort`. With normalization
        // it should emit an adaptive thinking block plus an
        // `output_config.effort` field. We only test 4.7 explicitly here; the
        // 4.6/4.5 variants share the same code path and are covered by the
        // Claude builder's own contract tests.
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.anthropic.claude-opus-4-7",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 4096,
            "reasoning_effort": "high",
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");
        assert_eq!(
            endpoint_path, "/model/us.anthropic.claude-opus-4-7/invoke",
            "URL path must retain the raw Bedrock id"
        );

        let body = body.as_object().expect("body should be object");
        let thinking = body
            .get("thinking")
            .and_then(Value::as_object)
            .expect("Opus 4.7 must emit an adaptive `thinking` block");
        assert_eq!(
            thinking.get("type").and_then(Value::as_str),
            Some("adaptive"),
            "Opus 4.7 thinking must be adaptive, got: {thinking:?}",
        );
        assert!(
            body.get("output_config")
                .and_then(Value::as_object)
                .and_then(|c| c.get("effort"))
                .is_some(),
            "Opus 4.7 must surface output_config.effort",
        );
    }

    #[test]
    fn bedrock_claude_maps_xhigh_by_normalized_model_contract() {
        let xhigh_payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.anthropic.claude-opus-4-7",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 4096,
            "reasoning_effort": "xhigh",
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_endpoint_path, body) = build(xhigh_payload).expect("payload should build");
        assert_eq!(
            body.pointer("/output_config/effort")
                .and_then(Value::as_str),
            Some("xhigh")
        );

        let max_payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "global.anthropic.claude-opus-4-6-v1",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 4096,
            "reasoning_effort": "xhigh",
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_endpoint_path, body) = build(max_payload).expect("payload should build");
        assert_eq!(
            body.pointer("/output_config/effort")
                .and_then(Value::as_str),
            Some("max")
        );
    }
}
