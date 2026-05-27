//! AWS Bedrock chat-completion payload dispatcher.
//!
//! Each Bedrock provider (Anthropic, Amazon Nova, Meta Llama, Mistral,
//! DeepSeek, Cohere Command R, AI21 Jamba, ...) has its own request body
//! schema, response shape and stream chunk encoding. This module owns:
//!
//! - Top-level provider detection from the Bedrock model id
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

mod ai21_jamba;
mod anthropic;
mod cohere;
mod custom;
mod deepseek;
mod llama;
mod mistral;
mod nova;
mod shared;

/// Inference-profile prefixes used by Bedrock cross-region routing.
/// Applies to every provider (Anthropic, Amazon Nova, Meta Llama, ...).
const BEDROCK_INFERENCE_PROFILE_PREFIXES: &[&str] =
    &["us.", "eu.", "apac.", "global.", "us-gov."];

/// Provider prefix attached by Bedrock catalog to bare foundation-model ids.
const BEDROCK_ANTHROPIC_PREFIX: &str = "anthropic.";

/// Bedrock model providers. The enum is intentionally exhaustive over the
/// providers we *recognize* — [`BedrockProvider::Other`] covers any prefix we
/// haven't explicitly wired a payload builder for yet (Titan, ...).
///
/// Adding a new provider here is a two-step PR: (1) extend [`detect_provider`]
/// to surface it, (2) add a `build_<provider>` branch to [`build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BedrockProvider {
    Anthropic,
    Nova,
    Llama,
    Mistral,
    DeepSeek,
    Cohere,
    Ai21Jamba,
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

    // Custom-template escape hatch: when explicitly enabled by the user,
    // bypass automatic provider dispatch entirely. This lets users wire
    // Bedrock-hosted models we don't have a first-class builder for yet
    // (Titan Text, Writer, Stability, future variants) without waiting on a
    // backend change.
    if custom::is_enabled(&payload) {
        return custom::build(payload, &model_id);
    }

    match detect_provider(&model_id) {
        BedrockProvider::Anthropic => anthropic::build(payload, &model_id),
        BedrockProvider::Nova => nova::build(payload, &model_id),
        BedrockProvider::Llama => llama::build(payload, &model_id),
        BedrockProvider::Mistral => mistral::build(payload, &model_id),
        BedrockProvider::DeepSeek => deepseek::build(payload, &model_id),
        BedrockProvider::Cohere => cohere::build(payload, &model_id),
        BedrockProvider::Ai21Jamba => ai21_jamba::build(payload, &model_id),
        BedrockProvider::Other(provider) => Err(ApplicationError::ValidationError(format!(
            "AWS Bedrock provider `{provider}` is not yet wired up by TauriTavern. Currently supported: Anthropic, Amazon Nova, Meta Llama, Mistral, DeepSeek, Cohere, AI21 Jamba."
        ))),
    }
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

    // Amazon's text-output foundation/inference-profile ids are exclusively
    // Nova (`amazon.nova-*`, `us.amazon.nova-pro-v1:0`, ...). Titan is gated
    // behind `text-output` filters in the catalog, so we don't see it here.
    if provider == "amazon" {
        return BedrockProvider::Nova;
    }

    if provider == "meta" {
        return BedrockProvider::Llama;
    }

    if provider == "mistral" {
        return BedrockProvider::Mistral;
    }

    if provider == "deepseek" {
        return BedrockProvider::DeepSeek;
    }

    if provider == "cohere" {
        return BedrockProvider::Cohere;
    }

    if provider == "ai21" && after_region.contains(".jamba") {
        return BedrockProvider::Ai21Jamba;
    }

    BedrockProvider::Other(provider.to_string())
}

/// Strip a Bedrock cross-region inference-profile prefix (`us.`, `eu.`,
/// `apac.`, `global.`, `us-gov.`) from a model id. Used by both
/// `detect_provider` and the Anthropic-specific id normalizer.
pub(super) fn strip_inference_profile_prefix(id: &str) -> &str {
    for prefix in BEDROCK_INFERENCE_PROFILE_PREFIXES {
        if let Some(rest) = id.strip_prefix(prefix) {
            return rest;
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{BedrockProvider, build, detect_provider};

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
    fn detect_provider_routes_amazon_models_to_nova_branch() {
        // Every Amazon text-output id in the Bedrock catalog is Nova today
        // (`amazon.nova-*` and its `us./global./apac.` inference-profile
        // mirrors). Titan is filtered out by `byOutputModality=TEXT` upstream.
        assert_eq!(
            detect_provider("us.amazon.nova-pro-v1:0"),
            BedrockProvider::Nova,
        );
        assert_eq!(
            detect_provider("amazon.nova-lite-v1:0"),
            BedrockProvider::Nova,
        );
        assert_eq!(
            detect_provider("amazon.nova-premier-v1:0"),
            BedrockProvider::Nova,
        );
    }

    #[test]
    fn detect_provider_routes_meta_models_to_llama_branch() {
        assert_eq!(
            detect_provider("meta.llama3-70b-instruct-v1:0"),
            BedrockProvider::Llama,
        );
        assert_eq!(
            detect_provider("us.meta.llama3-3-70b-instruct-v1:0"),
            BedrockProvider::Llama,
        );
        assert_eq!(
            detect_provider("meta.llama4-scout-17b-instruct-v1:0"),
            BedrockProvider::Llama,
        );
    }

    #[test]
    fn detect_provider_routes_mistral_models_to_mistral_branch() {
        assert_eq!(
            detect_provider("mistral.mistral-7b-instruct-v0:2"),
            BedrockProvider::Mistral,
        );
        assert_eq!(
            detect_provider("mistral.mixtral-8x7b-instruct-v0:1"),
            BedrockProvider::Mistral,
        );
        assert_eq!(
            detect_provider("mistral.mistral-large-2407-v1:0"),
            BedrockProvider::Mistral,
        );
        assert_eq!(
            detect_provider("us.mistral.pixtral-large-25-02-v1:0"),
            BedrockProvider::Mistral,
        );
    }

    #[test]
    fn detect_provider_routes_deepseek_models_to_deepseek_branch() {
        assert_eq!(
            detect_provider("deepseek.r1-v1:0"),
            BedrockProvider::DeepSeek,
        );
        assert_eq!(
            detect_provider("us.deepseek.r1-v1:0"),
            BedrockProvider::DeepSeek,
        );
        assert_eq!(
            detect_provider("deepseek.v3-v1:0"),
            BedrockProvider::DeepSeek,
        );
        assert_eq!(
            detect_provider("deepseek.v3.2"),
            BedrockProvider::DeepSeek,
        );
    }

    #[test]
    fn detect_provider_routes_cohere_models_to_cohere_branch() {
        assert_eq!(
            detect_provider("cohere.command-r-plus-v1:0"),
            BedrockProvider::Cohere,
        );
        assert_eq!(
            detect_provider("cohere.command-r-v1:0"),
            BedrockProvider::Cohere,
        );
        assert_eq!(
            detect_provider("us.cohere.command-r-plus-v1:0"),
            BedrockProvider::Cohere,
        );
    }

    #[test]
    fn detect_provider_routes_ai21_jamba_models_to_jamba_branch() {
        assert_eq!(
            detect_provider("ai21.jamba-instruct-v1:0"),
            BedrockProvider::Ai21Jamba,
        );
        assert_eq!(
            detect_provider("ai21.jamba-1-5-mini-v1:0"),
            BedrockProvider::Ai21Jamba,
        );
        assert_eq!(
            detect_provider("ai21.jamba-1-5-large-v1:0"),
            BedrockProvider::Ai21Jamba,
        );
        assert_eq!(
            detect_provider("us.ai21.jamba-1-5-large-v1:0"),
            BedrockProvider::Ai21Jamba,
        );
        // Legacy Jurassic models do not match `.jamba` and stay Other.
        assert_eq!(
            detect_provider("ai21.j2-ultra-v1"),
            BedrockProvider::Other("ai21".to_string()),
        );
    }

    #[test]
    fn detect_provider_surfaces_unsupported_providers_for_clear_error() {
        // Anything we haven't explicitly wired (Stability / Writer / Twelve
        // Labs / ...) keeps falling through as Other(<provider>) so callers
        // can still see a clear "not yet wired up" error.
        assert_eq!(
            detect_provider("stability.stable-diffusion-xl-v1"),
            BedrockProvider::Other("stability".to_string()),
        );
    }

    #[test]
    fn build_returns_clear_error_when_provider_is_not_yet_supported() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "stability.stable-diffusion-xl-v1",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let err = build(payload).expect_err("non-wired provider must fail with a clear error");
        let message = err.to_string();
        assert!(
            message.contains("not yet wired up"),
            "unexpected error: {message}",
        );
        assert!(
            message.contains("`stability`"),
            "error must name the unsupported provider: {message}",
        );
        assert!(
            message.contains("Anthropic"),
            "error must mention currently supported providers: {message}",
        );
    }
}
