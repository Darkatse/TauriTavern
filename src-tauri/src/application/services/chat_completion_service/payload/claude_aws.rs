use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::claude;

const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";
const BEDROCK_INVOKE_SUFFIX: &str = "invoke";

/// Inference-profile prefixes (cross-region routing) on Bedrock.
const BEDROCK_INFERENCE_PROFILE_PREFIXES: &[&str] = &["us.", "eu.", "apac.", "global.", "us-gov."];
/// Provider prefix attached by Bedrock catalog.
const BEDROCK_PROVIDER_PREFIX: &str = "anthropic.";

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let model_id = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "Claude on AWS Bedrock requires a model id (e.g. anthropic.claude-sonnet-4-20250514-v1:0)"
                    .to_string(),
            )
        })?;

    // Bedrock model IDs (e.g. `us.anthropic.claude-opus-4-7`,
    // `anthropic.claude-opus-4-6-v1`, `anthropic.claude-sonnet-4-5-20250929-v1:0`)
    // do NOT match the Anthropic-direct prefixes (`claude-opus-4-7`, `claude-opus-4-6`...)
    // that the Claude payload builder uses to resolve thinking / sampling / output-effort
    // capabilities. Normalize before delegating so model contract resolution works.
    let mut payload = payload;
    let normalized_model = normalize_bedrock_model_id(&model_id);
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

    let mut id = trimmed;
    for profile_prefix in BEDROCK_INFERENCE_PROFILE_PREFIXES {
        if let Some(rest) = id.strip_prefix(profile_prefix) {
            id = rest;
            break;
        }
    }
    if let Some(rest) = id.strip_prefix(BEDROCK_PROVIDER_PREFIX) {
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

    use super::{build, normalize_bedrock_model_id};

    #[test]
    fn bedrock_moves_model_to_url_path_and_injects_anthropic_version() {
        let payload = json!({
            "chat_completion_source": "claude_aws",
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
        assert!(body.get("model").is_none(), "model must be removed from body");
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
    fn bedrock_requires_model_id() {
        let payload = json!({
            "chat_completion_source": "claude_aws",
            "messages": [{ "role": "user", "content": "hello" }],
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let err = build(payload).expect_err("missing model should fail");
        assert!(
            err.to_string()
                .contains("Claude on AWS Bedrock requires a model id"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn bedrock_preserves_us_inference_profile_prefix_in_path() {
        let payload = json!({
            "chat_completion_source": "claude_aws",
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
        // Bare Bedrock catalog ids
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
        // Cross-region inference profile ids
        assert_eq!(
            normalize_bedrock_model_id("us.anthropic.claude-opus-4-7"),
            "claude-opus-4-7"
        );
        assert_eq!(
            normalize_bedrock_model_id("global.anthropic.claude-opus-4-6-v1"),
            "claude-opus-4-6"
        );
        assert_eq!(
            normalize_bedrock_model_id("eu.anthropic.claude-haiku-4-5-20251001-v1:0"),
            "claude-haiku-4-5-20251001"
        );
        // Already-normalized ids pass through unchanged
        assert_eq!(
            normalize_bedrock_model_id("claude-3-5-sonnet-20240620"),
            "claude-3-5-sonnet-20240620"
        );
        // Padding tolerance
        assert_eq!(
            normalize_bedrock_model_id("  us.anthropic.claude-opus-4-7  "),
            "claude-opus-4-7"
        );
    }

    #[test]
    fn bedrock_unlocks_opus_4_7_adaptive_thinking_via_normalization() {
        // Without normalization, `ClaudeModelContract::resolve` cannot
        // match `claude-opus-4-7` against `us.anthropic.claude-opus-4-7`,
        // so the builder would silently strip `reasoning_effort`. With
        // normalization it should emit an adaptive thinking block plus an
        // `output_config.effort` field.
        let payload = json!({
            "chat_completion_source": "claude_aws",
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
            endpoint_path,
            "/model/us.anthropic.claude-opus-4-7/invoke",
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
            thinking.get("budget_tokens").is_none(),
            "adaptive thinking must not pin budget_tokens, got: {thinking:?}",
        );

        let output_config = body
            .get("output_config")
            .and_then(Value::as_object)
            .expect("Opus 4.7 must surface `output_config.effort`");
        assert!(
            output_config.get("effort").is_some(),
            "output_config must carry effort, got: {output_config:?}",
        );
    }

    #[test]
    fn bedrock_unlocks_opus_4_6_adaptive_thinking_via_normalization() {
        // Opus 4.6 is `ManualOrAdaptive`, which in this builder also emits the
        // adaptive thinking shape plus `output_config.effort`.
        let payload = json!({
            "chat_completion_source": "claude_aws",
            "model": "anthropic.claude-opus-4-6-v1",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 4096,
            "reasoning_effort": "medium",
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_endpoint_path, body) = build(payload).expect("payload should build");
        let body = body.as_object().expect("body should be object");
        let thinking = body
            .get("thinking")
            .and_then(Value::as_object)
            .expect("Opus 4.6 must emit a `thinking` block");
        assert_eq!(
            thinking.get("type").and_then(Value::as_str),
            Some("adaptive"),
        );

        assert!(
            body.get("output_config")
                .and_then(Value::as_object)
                .and_then(|c| c.get("effort"))
                .is_some(),
            "Opus 4.6 must surface output_config.effort",
        );
    }

    #[test]
    fn bedrock_unlocks_opus_4_5_manual_thinking_via_normalization() {
        // Opus 4.5 is `ManualOnly`, which uses the explicit budget_tokens
        // thinking block.
        let payload = json!({
            "chat_completion_source": "claude_aws",
            "model": "anthropic.claude-opus-4-5-20251101-v1:0",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 4096,
            "reasoning_effort": "low",
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_endpoint_path, body) = build(payload).expect("payload should build");
        let body = body.as_object().expect("body should be object");
        let thinking = body
            .get("thinking")
            .and_then(Value::as_object)
            .expect("Opus 4.5 must emit a `thinking` block");
        assert_eq!(
            thinking.get("type").and_then(Value::as_str),
            Some("enabled"),
        );
        assert!(
            thinking.get("budget_tokens").is_some(),
            "Opus 4.5 manual thinking must carry budget_tokens, got: {thinking:?}",
        );
    }
}
