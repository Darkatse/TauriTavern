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
    Llama,
    Mistral,
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
        BedrockProvider::Llama => build_llama(payload, &model_id),
        BedrockProvider::Mistral => build_mistral(payload, &model_id),
        BedrockProvider::Other(provider) => Err(ApplicationError::ValidationError(format!(
            "AWS Bedrock provider `{provider}` is not yet wired up by TauriTavern. Currently supported: Anthropic, Amazon Nova, Meta Llama, Mistral. Other providers (Cohere, AI21, DeepSeek) will be added in follow-up releases."
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

/// Build a Meta Llama invoke body. Llama 3 / 3.1 / 3.2 / 3.3 / 4 all share the
/// same chat template format on Bedrock's `/model/{id}/invoke` endpoint:
///
/// ```text
/// <|begin_of_text|>
/// <|start_header_id|>system<|end_header_id|>
///
/// {system}<|eot_id|>
/// <|start_header_id|>user<|end_header_id|>
///
/// {user}<|eot_id|>
/// <|start_header_id|>assistant<|end_header_id|>
///
/// ```
///
/// Request body (per AWS Bedrock User Guide `model-parameters-meta.md`):
/// ```json
/// { "prompt": "<|begin_of_text|>...", "max_gen_len": 512, "temperature": 0.5, "top_p": 0.9 }
/// ```
///
/// Non-stream response:
/// ```json
/// { "generation": "...", "prompt_token_count": N, "generation_token_count": M, "stop_reason": "stop" }
/// ```
///
/// Stream chunk (one decoded EventStream frame per token group):
/// ```json
/// { "generation": "...", "prompt_token_count": ..., "generation_token_count": ..., "stop_reason": null|"stop" }
/// ```
fn build_llama(
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

/// Build a Mistral invoke body. Bedrock hosts two distinct Mistral schemas
/// that share neither request nor response shapes:
///
/// 1. **Text-completion (pre-2407 / Mixtral / 7B)** — `prompt`-based with the
///    `<s>[INST] ... [/INST]` instruct template.
///    Request: `{ "prompt": "...", "max_tokens": N, "temperature": ..., "top_p": ..., "top_k": ... }`
///    Response: `{ "outputs": [{ "text": "...", "stop_reason": "..." }] }`
///    Stream chunk: `{ "outputs": [{ "text": "...", "stop_reason": null|"stop" }] }`
///
/// 2. **Chat-completion (mistral-large-2407+, Mistral Small / Medium / Pixtral)** —
///    OpenAI-style `messages` + `tools`.
///    Request: `{ "messages": [{"role":"user","content":"..."}], "max_tokens": ..., "temperature": ..., "top_p": ..., "tools": [...] }`
///    Response (mistral-large-2407 doc): `{ "content": [{ "role": "assistant", "content": [{ "text": "..." }] }] }`
///    Response (mistral-chat-completion doc): `{ "choices": [{ "index":0, "message":{"role":"assistant","content":"string"}, "stop_reason":"stop" }] }`
///    Stream chunk (mistral-large-2407): `{ "content": [{ "text": "..." }] }`
///    Stream chunk (chat-completion): `{ "choices": [{ "delta": { "content": "..." } }] }`
///
/// Dispatch is driven by the Bedrock model id: anything matching
/// `mistral-7b`, `mixtral`, or the pre-2407 `-2402` cohort takes the legacy
/// text-completion path; everything else (2407+, large, small, medium,
/// pixtral, ...) is treated as chat.
fn build_mistral(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let endpoint_path = format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}");

    if is_legacy_mistral_text_completion(model_id) {
        return Ok((endpoint_path, build_mistral_text_completion_body(payload)));
    }

    Ok((endpoint_path, build_mistral_chat_completion_body(payload)))
}

fn is_legacy_mistral_text_completion(model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();
    // Cross-region prefix has already been preserved in `model_id`; substring
    // match works regardless of `us.` / `eu.` / ... profile prefixes.
    id.contains("mistral-7b") || id.contains("mixtral") || id.contains("-2402")
}

fn build_mistral_text_completion_body(payload: Map<String, Value>) -> Value {
    let (system_text, conversation) = flatten_openai_messages(payload.get("messages"));
    let prompt = format_mistral_instruct_prompt(system_text.as_deref(), &conversation);

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

fn build_mistral_chat_completion_body(payload: Map<String, Value>) -> Value {
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
    if let Some(tools) = payload.get("tools").cloned().filter(|value| value.is_array()) {
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
pub(super) fn format_mistral_instruct_prompt(
    system: Option<&str>,
    turns: &[FlatMessage],
) -> String {
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

/// Pass an OpenAI-shape messages array through to the Mistral chat-completion
/// endpoint mostly verbatim. We collapse multi-part content into plain text
/// because the Bedrock-side schema does not document a chat content-block
/// format on the `/invoke` endpoint (multimodal goes through the Converse
/// API).
pub(super) fn passthrough_chat_messages(messages: Option<&Value>) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(messages) = messages else { return out };

    if let Some(prompt) = messages.as_str() {
        out.push(json!({ "role": "user", "content": prompt }));
        return out;
    }

    let Some(entries) = messages.as_array() else {
        return out;
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
        let role = match role.as_str() {
            "system" | "developer" => "system",
            "assistant" => "assistant",
            "tool" | "function" => "user",
            _ => "user",
        };
        let text = message_content_to_text(message.get("content"));
        if text.is_empty() {
            continue;
        }
        out.push(json!({ "role": role, "content": text }));
    }

    out
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

    if provider == "meta" {
        return BedrockProvider::Llama;
    }

    if provider == "mistral" {
        return BedrockProvider::Mistral;
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
        format_llama3_chat_prompt, normalize_bedrock_model_id,
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
    fn detect_provider_surfaces_unsupported_providers_for_clear_error() {
        // Cohere, AI21, DeepSeek all surface as Other(<provider>) until
        // follow-up PRs wire builders.
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
            "model": "cohere.command-r-plus-v1:0",
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
            message.contains("`cohere`"),
            "error must name the unsupported provider: {message}",
        );
        assert!(
            message.contains("Anthropic"),
            "error must mention currently supported providers: {message}",
        );
    }

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
        assert!(body.get("messages").is_none(), "llama uses prompt, not messages");
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
        assert_eq!(endpoint_path, "/model/mistral.mistral-7b-instruct-v0:2/invoke");
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
        assert_eq!(endpoint_path, "/model/mistral.mistral-large-2407-v1:0/invoke");
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
        assert!(body.get("prompt").is_none(), "chat path must not emit a prompt");
        assert_eq!(body.get("max_tokens").and_then(Value::as_i64), Some(1024));
        assert_eq!(body.get("temperature").and_then(Value::as_f64), Some(0.4));
        assert!(body.get("tools").is_some(), "tools array passes through");
    }

    #[test]
    fn format_llama3_chat_prompt_handles_multi_turn_without_system_message() {
        let prompt = format_llama3_chat_prompt(
            None,
            &[
                FlatMessage { role: "user".to_string(), text: "hi".to_string() },
                FlatMessage {
                    role: "assistant".to_string(),
                    text: "hello".to_string(),
                },
                FlatMessage { role: "user".to_string(), text: "again".to_string() },
            ],
        );

        assert!(prompt.starts_with("<|begin_of_text|><|start_header_id|>user<|end_header_id|>"));
        assert!(prompt.contains("<|start_header_id|>assistant<|end_header_id|>\n\nhello<|eot_id|>"));
        assert!(prompt.contains("<|start_header_id|>user<|end_header_id|>\n\nagain<|eot_id|>"));
        assert!(prompt.ends_with("<|start_header_id|>assistant<|end_header_id|>\n\n"));
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
