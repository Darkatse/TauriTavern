use serde_json::{Map, Number, Value, json};

use crate::application::errors::ApplicationError;

use super::claude;
use super::shared::message_content_to_text;

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
/// haven't explicitly wired a payload builder for yet (Titan, ...).
///
/// Adding a new provider here is a two-step PR: (1) extend [`detect_provider`]
/// to surface it, (2) add a `build_<provider>` branch to [`build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BedrockProvider {
    Anthropic,
    Nova,
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
        BedrockProvider::Nova => build_nova(payload, &model_id),
        BedrockProvider::Other(provider) => Err(ApplicationError::ValidationError(format!(
            "AWS Bedrock provider `{provider}` is not yet wired up by TauriTavern. Currently supported: Anthropic, Amazon Nova. Other providers (Meta Llama, Mistral, Cohere, AI21, DeepSeek) will be added in follow-up releases."
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

/// Build an Amazon Nova invoke body. Nova accepts the *same* schema on
/// `/model/{id}/invoke` that the Converse API uses; the chunks coming back on
/// `/invoke-with-response-stream` are Converse-style events (`messageStart`,
/// `contentBlockDelta`, `messageStop`, `metadata`).
///
/// Request body shape (per AWS Bedrock User Guide — `model-card-amazon-nova-*`,
/// `prompt-caching.md`, and the Converse API mapping):
/// ```json
/// {
///   "system": [{ "text": "..." }],
///   "messages": [
///     { "role": "user", "content": [{ "text": "..." }] }
///   ],
///   "inferenceConfig": { "maxTokens": 300, "topP": 0.1, "topK": 20, "temperature": 0.3 }
/// }
/// ```
///
/// Non-stream response:
/// ```json
/// { "output": { "message": { "role": "assistant", "content": [{ "text": "..." }] } },
///   "stopReason": "end_turn", "usage": { "inputTokens": N, "outputTokens": M } }
/// ```
///
/// Stream chunk (decoded base64 bytes of each EventStream frame):
/// ```json
/// { "contentBlockDelta": { "delta": { "text": "..." }, "contentBlockIndex": 0 } }
/// ```
fn build_nova(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let (system_text, conversation) = flatten_openai_messages(payload.get("messages"));

    let nova_messages: Vec<Value> = conversation
        .into_iter()
        .map(|FlatMessage { role, text }| {
            let role = if role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            json!({
                "role": role,
                "content": [{ "text": text }],
            })
        })
        .collect();

    let mut body = Map::new();
    body.insert("messages".to_string(), Value::Array(nova_messages));

    if let Some(text) = system_text.filter(|value| !value.is_empty()) {
        body.insert(
            "system".to_string(),
            Value::Array(vec![json!({ "text": text })]),
        );
    }

    let inference_config = build_nova_inference_config(&payload);
    if !inference_config.is_empty() {
        body.insert("inferenceConfig".to_string(), Value::Object(inference_config));
    }

    Ok((
        format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}"),
        Value::Object(body),
    ))
}

fn build_nova_inference_config(payload: &Map<String, Value>) -> Map<String, Value> {
    let mut config = Map::new();

    if let Some(max_tokens) = value_to_positive_i64(payload.get("max_tokens")) {
        config.insert("maxTokens".to_string(), Value::Number(Number::from(max_tokens)));
    }
    if let Some(temperature) = payload.get("temperature").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(temperature) {
            config.insert("temperature".to_string(), Value::Number(number));
        }
    }
    if let Some(top_p) = payload.get("top_p").and_then(Value::as_f64) {
        if let Some(number) = Number::from_f64(top_p) {
            config.insert("topP".to_string(), Value::Number(number));
        }
    }
    if let Some(top_k) = value_to_positive_i64(payload.get("top_k")) {
        config.insert("topK".to_string(), Value::Number(Number::from(top_k)));
    }
    if let Some(stop) = payload.get("stop").cloned().filter(|value| !value.is_null()) {
        // Bedrock Converse-style payload accepts `stopSequences`.
        let stops = match stop {
            Value::Array(values) => values
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>(),
            Value::String(value) => vec![value],
            _ => Vec::new(),
        };
        if !stops.is_empty() {
            config.insert(
                "stopSequences".to_string(),
                Value::Array(stops.into_iter().map(Value::String).collect()),
            );
        }
    }

    config
}

/// Lightweight per-message representation used by every non-Anthropic builder.
/// Provider-native shapes (Nova content blocks, Llama prompt template, Cohere
/// chat_history, ...) are reconstructed from this flat list in each builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlatMessage {
    pub role: String,
    pub text: String,
}

/// Flatten an OpenAI-style `messages` array into a `(system_text, [user/assistant turns])`
/// pair. System messages are concatenated with `\n\n` and pulled out of the
/// conversation list — every Bedrock non-Anthropic provider treats `system` as
/// a separate top-level field (Nova `system`, Llama `<|start_header_id|>system`,
/// Cohere `preamble`, AI21 `role:"system"`, DeepSeek prompt prefix, ...).
///
/// `tool` role messages are demoted to `user` text with a `[tool_result] ...`
/// envelope. We could later add per-provider tool-call wiring, but for the
/// initial multi-provider release we focus on plain chat.
pub(super) fn flatten_openai_messages(
    messages: Option<&Value>,
) -> (Option<String>, Vec<FlatMessage>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut turns: Vec<FlatMessage> = Vec::new();

    let Some(messages) = messages else {
        return (None, turns);
    };

    if let Some(prompt) = messages.as_str() {
        turns.push(FlatMessage {
            role: "user".to_string(),
            text: prompt.to_string(),
        });
        return (None, turns);
    }

    let Some(entries) = messages.as_array() else {
        return (None, turns);
    };

    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_lowercase();
        let text = message_content_to_text(message.get("content"));
        if text.is_empty() {
            continue;
        }

        match role.as_str() {
            "system" | "developer" => system_parts.push(text),
            "tool" | "function" => turns.push(FlatMessage {
                role: "user".to_string(),
                text: format!("[tool_result] {text}"),
            }),
            "assistant" => turns.push(FlatMessage {
                role: "assistant".to_string(),
                text,
            }),
            _ => turns.push(FlatMessage {
                role: "user".to_string(),
                text,
            }),
        }
    }

    let system_text = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    (system_text, turns)
}

fn value_to_positive_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64).filter(|number| *number > 0)
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

    use super::{
        BedrockProvider, FlatMessage, build, detect_provider, flatten_openai_messages,
        normalize_bedrock_model_id,
    };

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
    fn detect_provider_surfaces_unsupported_providers_for_clear_error() {
        // Meta Llama, Mistral, Cohere, AI21, DeepSeek all surface as
        // Other(<provider>) until follow-up PRs wire builders.
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
            "model": "us.meta.llama3-3-70b-instruct-v1:0",
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
            message.contains("`meta`"),
            "error must name the unsupported provider: {message}",
        );
        assert!(
            message.contains("Anthropic"),
            "error must mention currently supported providers: {message}",
        );
    }

    #[test]
    fn build_nova_emits_converse_style_invoke_body_for_inference_profile() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "us.amazon.nova-pro-v1:0",
            "messages": [
                { "role": "system", "content": "be concise" },
                { "role": "user", "content": "hello" }
            ],
            "max_tokens": 256,
            "temperature": 0.4,
            "top_p": 0.9,
            "top_k": 50,
            "stop": ["###"],
            "stream": true,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");

        assert_eq!(endpoint_path, "/model/us.amazon.nova-pro-v1:0/invoke");
        let body = body.as_object().expect("body should be object");

        // Body must not leak the routing-only fields.
        assert!(body.get("model").is_none());
        assert!(body.get("stream").is_none());

        let system = body
            .get("system")
            .and_then(Value::as_array)
            .expect("nova must lift system messages out of the conversation");
        assert_eq!(
            system[0].get("text").and_then(Value::as_str),
            Some("be concise"),
        );

        let messages = body.get("messages").and_then(Value::as_array).expect("messages");
        assert_eq!(messages.len(), 1, "system was lifted out");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"][0]["text"], "hello");

        let inference = body
            .get("inferenceConfig")
            .and_then(Value::as_object)
            .expect("nova must always carry an inferenceConfig when params are present");
        assert_eq!(inference.get("maxTokens").and_then(Value::as_i64), Some(256));
        assert_eq!(inference.get("temperature").and_then(Value::as_f64), Some(0.4));
        assert_eq!(inference.get("topP").and_then(Value::as_f64), Some(0.9));
        assert_eq!(inference.get("topK").and_then(Value::as_i64), Some(50));
        let stop = inference
            .get("stopSequences")
            .and_then(Value::as_array)
            .expect("stopSequences");
        assert_eq!(stop[0], "###");
    }

    #[test]
    fn flatten_openai_messages_extracts_system_and_normalizes_roles() {
        let messages = json!([
            { "role": "system", "content": "rules apply" },
            { "role": "developer", "content": "extra system" },
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": "hello" },
            { "role": "tool", "content": "tool result" }
        ]);

        let (system, turns) = flatten_openai_messages(Some(&messages));
        assert_eq!(system.as_deref(), Some("rules apply\n\nextra system"));
        assert_eq!(
            turns,
            vec![
                FlatMessage { role: "user".to_string(), text: "hi".to_string() },
                FlatMessage { role: "assistant".to_string(), text: "hello".to_string() },
                FlatMessage {
                    role: "user".to_string(),
                    text: "[tool_result] tool result".to_string(),
                },
            ]
        );
    }

    #[test]
    fn build_nova_falls_back_to_user_role_when_no_system_messages_present() {
        let payload = json!({
            "chat_completion_source": "aws_bedrock",
            "model": "amazon.nova-micro-v1:0",
            "messages": [{ "role": "user", "content": "hi" }],
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_, body) = build(payload).expect("payload should build");
        let body = body.as_object().expect("body should be object");
        assert!(
            body.get("system").is_none(),
            "system block should be omitted when no system messages exist",
        );
        assert_eq!(
            body.get("messages")
                .and_then(Value::as_array)
                .map(|messages| messages.len()),
            Some(1),
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
