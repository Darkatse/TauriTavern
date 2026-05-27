use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::claude;

const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";
const BEDROCK_INVOKE_SUFFIX: &str = "invoke";

/// Inference-profile prefixes used by Bedrock cross-region routing.
/// Applies to every provider (Anthropic, Amazon Nova, Meta Llama, ...).
const BEDROCK_INFERENCE_PROFILE_PREFIXES: &[&str] =
    &["us.", "eu.", "apac.", "global.", "us-gov."];

/// Provider prefix attached by Bedrock catalog to bare foundation-model ids.
const BEDROCK_ANTHROPIC_PREFIX: &str = "anthropic.";

/// Bedrock model providers. The enum is intentionally exhaustive over the
/// providers we *recognize* — [`BedrockProvider::Other`] covers any prefix we
/// haven't explicitly wired a payload builder for yet (Amazon Nova/Titan,
/// Meta Llama, Mistral, Cohere, AI21, DeepSeek, etc.).
///
/// Adding a new provider here is a two-step PR: (1) extend [`detect_provider`]
/// to surface it, (2) add a `build_<provider>` branch to [`build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BedrockProvider {
    Anthropic,
    Other(String),
}

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

    match detect_provider(&model_id) {
        BedrockProvider::Anthropic => build_anthropic(payload, &model_id),
        BedrockProvider::Other(provider) => Err(ApplicationError::ValidationError(format!(
            "AWS Bedrock provider `{provider}` is not yet wired up by TauriTavern. Currently supported: Anthropic. Pick a model id starting with `anthropic.` or any inference profile that ends in `*.anthropic.*` (e.g. us.anthropic.claude-opus-4-7). Other providers (Amazon Nova/Titan, Meta Llama, Mistral, Cohere, AI21, DeepSeek) will be added in follow-up releases."
        ))),
    }
}

/// Build an Anthropic Messages payload by delegating to [`claude::build`] and
/// rewriting the result for Bedrock's `/model/{modelId}/invoke` endpoint.
fn build_anthropic(
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

/// Detect which Bedrock provider a model id belongs to by stripping any
/// cross-region inference-profile prefix and inspecting the next segment
/// (`anthropic.` / `amazon.` / `meta.` / `mistral.` / `cohere.` / `ai21.` /
/// `deepseek.` / ...).
pub(super) fn detect_provider(model_id: &str) -> BedrockProvider {
    let after_region = strip_inference_profile_prefix(model_id.trim());
    if after_region.starts_with(BEDROCK_ANTHROPIC_PREFIX) {
        return BedrockProvider::Anthropic;
    }
    let provider = after_region
        .split('.')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or("unknown");
    BedrockProvider::Other(provider.to_string())
}

fn strip_inference_profile_prefix(id: &str) -> &str {
    for prefix in BEDROCK_INFERENCE_PROFILE_PREFIXES {
        if let Some(rest) = id.strip_prefix(prefix) {
            return rest;
        }
    }
    id
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

    use super::{BedrockProvider, build, detect_provider, normalize_bedrock_model_id};

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
    fn detect_provider_identifies_anthropic_models_and_inference_profiles() {
        assert_eq!(
            detect_provider("anthropic.claude-opus-4-7"),
            BedrockProvider::Anthropic,
        );
        assert_eq!(
            detect_provider("us.anthropic.claude-opus-4-7"),
            BedrockProvider::Anthropic,
        );
        assert_eq!(
            detect_provider("global.anthropic.claude-opus-4-6-v1"),
            BedrockProvider::Anthropic,
        );
    }

    #[test]
    fn detect_provider_surfaces_unsupported_providers_for_clear_error() {
        // Amazon Nova / Titan, Meta Llama, Mistral, Cohere, AI21, DeepSeek
        // all surface as Other(<provider>) until follow-up PRs wire builders.
        assert_eq!(
            detect_provider("us.amazon.nova-pro-v1:0"),
            BedrockProvider::Other("amazon".to_string()),
        );
        assert_eq!(
            detect_provider("amazon.titan-text-premier-v1:0"),
            BedrockProvider::Other("amazon".to_string()),
        );
        assert_eq!(
            detect_provider("us.meta.llama3-3-70b-instruct-v1:0"),
            BedrockProvider::Other("meta".to_string()),
        );
        assert_eq!(
            detect_provider("mistral.mistral-large-2407-v1:0"),
            BedrockProvider::Other("mistral".to_string()),
        );
        assert_eq!(
            detect_provider("cohere.command-r-plus-v1:0"),
            BedrockProvider::Other("cohere".to_string()),
        );
        assert_eq!(
            detect_provider("ai21.jamba-1-5-large-v1:0"),
            BedrockProvider::Other("ai21".to_string()),
        );
        assert_eq!(
            detect_provider("deepseek.r1-v1:0"),
            BedrockProvider::Other("deepseek".to_string()),
        );
    }

    #[test]
    fn build_returns_clear_error_when_provider_is_not_yet_supported() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.amazon.nova-pro-v1:0",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let err = build(payload).expect_err("non-Anthropic provider must fail with a clear error");
        let message = err.to_string();
        assert!(
            message.contains("not yet wired up"),
            "unexpected error: {message}",
        );
        assert!(
            message.contains("`amazon`"),
            "error must name the unsupported provider: {message}",
        );
        assert!(
            message.contains("anthropic."),
            "error must point users at the supported prefix: {message}",
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
            body.get("output_config")
                .and_then(Value::as_object)
                .and_then(|c| c.get("effort"))
                .is_some(),
            "Opus 4.7 must surface output_config.effort",
        );
    }
}
