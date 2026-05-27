use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Map, Value, json};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;
use super::response_body::read_upstream_json_body;

const BEDROCK_PROVIDER_NAME: &str = "AWS Bedrock";
const BEDROCK_EVENTSTREAM_CONTENT_TYPE: &str = "application/vnd.amazon.eventstream";
const BEDROCK_INVOKE_SUFFIX: &str = "/invoke";
const BEDROCK_STREAM_SUFFIX: &str = "/invoke-with-response-stream";
const BEDROCK_RUNTIME_HOST_INFIX: &str = "bedrock-runtime.";
const BEDROCK_CONTROL_PLANE_HOST_INFIX: &str = "bedrock.";

/// Inference-profile prefixes used by Bedrock cross-region routing. Used to
/// extract the underlying provider name from an inference-profile id (e.g.
/// `us.anthropic.claude-opus-4-7` -> provider `anthropic`).
const BEDROCK_INFERENCE_PROFILE_PREFIXES: &[&str] =
    &["us.", "eu.", "apac.", "global.", "us-gov."];

/// HTTP-side view of which Bedrock provider a request belongs to. Used to
/// pick the right non-stream response normalizer and the right stream-chunk
/// transformer. Kept narrowly here (separate from
/// `payload::aws_bedrock::BedrockProvider`) so the infrastructure layer
/// doesn't import from the application layer (Clean Architecture rule:
/// dependencies point inward).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpBedrockProvider {
    Anthropic,
    Nova,
    Llama,
    Mistral,
    DeepSeek,
    Cohere,
    Ai21Jamba,
    Other,
}

fn detect_http_provider(model_id: &str) -> HttpBedrockProvider {
    match extract_provider(model_id) {
        "anthropic" => HttpBedrockProvider::Anthropic,
        "amazon" => HttpBedrockProvider::Nova,
        "meta" => HttpBedrockProvider::Llama,
        "mistral" => HttpBedrockProvider::Mistral,
        "deepseek" => HttpBedrockProvider::DeepSeek,
        "cohere" => HttpBedrockProvider::Cohere,
        // Jurassic-2 (`ai21.j2-*`) doesn't match `.jamba` and stays Other.
        "ai21" if model_id.contains(".jamba") => HttpBedrockProvider::Ai21Jamba,
        _ => HttpBedrockProvider::Other,
    }
}

/// Pull the model id out of a Bedrock invoke endpoint path. Returns an empty
/// string when the path is malformed (the caller falls back to the Claude
/// normalizer in that case, which is harmless: the request would already have
/// failed at the application layer).
fn extract_model_id_from_endpoint(endpoint_path: &str) -> &str {
    let Some(rest) = endpoint_path.strip_prefix("/model/") else {
        return "";
    };
    rest.rsplit_once('/').map(|(model, _)| model).unwrap_or("")
}

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let control_plane_base = derive_control_plane_base(&config.base_url)?;
    // Ask the control plane for *every* text-output foundation model the
    // account has access to. Bedrock supports filtering by provider, but
    // since TauriTavern wants to surface the entire catalog (with a
    // best-effort `(unsupported)` tag in the UI for providers we haven't
    // wired payload builders for yet), we drop the byProvider filter.
    let foundation_url =
        format!("{control_plane_base}/foundation-models?byOutputModality=TEXT");
    let profiles_url = format!("{control_plane_base}/inference-profiles");

    let client = repository.client()?;
    // Doing the two calls in sequence (rather than `tokio::try_join!`) keeps
    // the dependency graph small and matters very little here: each call is a
    // small JSON GET against the regional control plane.
    let foundation = get_control_plane_json(&client, config, &foundation_url, "foundation-models")
        .await?;
    let profiles = get_control_plane_json(&client, config, &profiles_url, "inference-profiles")
        .await?;

    Ok(json!({ "data": merge_bedrock_models(&foundation, &profiles) }))
}

fn derive_control_plane_base(runtime_base: &str) -> Result<String, DomainError> {
    if let Some(rest) = runtime_base.split_once(BEDROCK_RUNTIME_HOST_INFIX) {
        let (scheme, suffix) = rest;
        return Ok(format!(
            "{scheme}{BEDROCK_CONTROL_PLANE_HOST_INFIX}{suffix}"
        ));
    }
    if runtime_base.contains(BEDROCK_CONTROL_PLANE_HOST_INFIX) {
        return Ok(runtime_base.to_string());
    }
    Err(DomainError::InvalidData(format!(
        "Cannot derive Bedrock control-plane URL from base `{runtime_base}`",
    )))
}

async fn get_control_plane_json(
    client: &reqwest::Client,
    config: &ChatCompletionApiConfig,
    url: &str,
    op: &str,
) -> Result<Value, DomainError> {
    let request = client.get(url).header(ACCEPT, "application/json");
    let request = apply_bedrock_auth(request, config);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error(
            &format!("{BEDROCK_PROVIDER_NAME} {op} request failed"),
            error,
        )
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            BEDROCK_PROVIDER_NAME,
            response,
            &format!("Failed to list Bedrock {op}"),
        )
        .await);
    }

    read_upstream_json_body(BEDROCK_PROVIDER_NAME, op, response).await
}

/// Returns whether a Bedrock foundation-model entry can be invoked directly
/// (without an inference profile). Bedrock's catalog exposes this through
/// `inferenceTypesSupported`, which is an array containing some subset of
/// `["ON_DEMAND", "PROVISIONED", "INFERENCE_PROFILE"]`. Missing/empty arrays
/// are treated as opt-in (we surface the entry) to stay forward-compatible
/// with future fields.
fn inference_supports_on_demand(model_summary: &Value) -> bool {
    let Some(arr) = model_summary
        .get("inferenceTypesSupported")
        .and_then(Value::as_array)
    else {
        return true;
    };
    if arr.is_empty() {
        return true;
    }
    arr.iter()
        .any(|value| value.as_str() == Some("ON_DEMAND"))
}

/// Extract the provider name from a Bedrock model id by stripping any
/// inference-profile prefix (us./eu./apac./global./us-gov.) and returning
/// the leading dotted segment. Examples:
/// - `anthropic.claude-3-haiku`         -> `anthropic`
/// - `us.anthropic.claude-opus-4-7`     -> `anthropic`
/// - `amazon.nova-pro-v1:0`             -> `amazon`
/// - `us.meta.llama3-3-70b-instruct`    -> `meta`
fn extract_provider(id: &str) -> &str {
    let after_region = BEDROCK_INFERENCE_PROFILE_PREFIXES
        .iter()
        .find_map(|prefix| id.strip_prefix(prefix))
        .unwrap_or(id);
    after_region.split('.').next().unwrap_or(after_region)
}

fn merge_bedrock_models(foundation: &Value, profiles: &Value) -> Vec<Value> {
    let mut entries: Vec<Value> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    if let Some(items) = foundation
        .get("modelSummaries")
        .and_then(Value::as_array)
    {
        for item in items {
            // Skip retired models when the catalog marks them as such.
            let status = item
                .get("modelLifecycle")
                .and_then(|m| m.get("status"))
                .and_then(Value::as_str);
            if matches!(status, Some(s) if s != "ACTIVE") {
                continue;
            }
            // AWS Claude 4.x and many newer Nova/Llama foundation models are
            // tagged `INFERENCE_PROFILE` only — invoking them with the raw
            // foundation-model id is rejected by Bedrock with
            //   "Invocation of model ID ... with on-demand throughput isn't supported.
            //    Retry your request with the ID or ARN of an inference profile..."
            // So we hide foundation entries that don't support ON_DEMAND from
            // the dropdown; their `us./global.` inference-profile variants come
            // back through the second response below and are surfaced instead.
            if !inference_supports_on_demand(item) {
                continue;
            }
            let Some(id) = item.get("modelId").and_then(Value::as_str) else {
                continue;
            };
            if !seen.insert(id.to_string()) {
                continue;
            }
            entries.push(json!({
                "id": id,
                "name": item.get("modelName").cloned().unwrap_or(Value::Null),
                "source": "foundation-model",
                "provider": extract_provider(id),
            }));
        }
    }

    if let Some(items) = profiles
        .get("inferenceProfileSummaries")
        .and_then(Value::as_array)
    {
        for item in items {
            let status = item.get("status").and_then(Value::as_str);
            if !matches!(status, Some("ACTIVE")) {
                continue;
            }
            let Some(id) = item.get("inferenceProfileId").and_then(Value::as_str) else {
                continue;
            };
            if !seen.insert(id.to_string()) {
                continue;
            }
            entries.push(json!({
                "id": id,
                "name": item.get("inferenceProfileName").cloned().unwrap_or(Value::Null),
                "source": "inference-profile",
                "provider": extract_provider(id),
            }));
        }
    }

    entries
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    validate_invoke_endpoint(endpoint_path)?;
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(payload);
    let request = apply_bedrock_auth(request, config);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            BEDROCK_PROVIDER_NAME,
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = read_upstream_json_body(BEDROCK_PROVIDER_NAME, "generate", response).await?;
    Ok(normalize_provider_response(endpoint_path, body))
}

fn normalize_provider_response(
    endpoint_path: &str,
    body: Value,
) -> ChatCompletionRepositoryGenerateResponse {
    let model_id = extract_model_id_from_endpoint(endpoint_path);
    match detect_http_provider(model_id) {
        HttpBedrockProvider::Anthropic => normalizers::normalize_claude_response(body),
        HttpBedrockProvider::Nova => {
            normalizers::normalize_claude_response(nova_response_to_claude_shape(body))
        }
        HttpBedrockProvider::Llama => {
            normalizers::normalize_claude_response(llama_response_to_claude_shape(body))
        }
        HttpBedrockProvider::Mistral => {
            normalizers::normalize_claude_response(mistral_response_to_claude_shape(body))
        }
        HttpBedrockProvider::DeepSeek => {
            normalizers::normalize_claude_response(deepseek_response_to_claude_shape(body))
        }
        HttpBedrockProvider::Cohere => {
            normalizers::normalize_claude_response(cohere_response_to_claude_shape(body))
        }
        HttpBedrockProvider::Ai21Jamba => {
            normalizers::normalize_claude_response(ai21_jamba_response_to_claude_shape(body))
        }
        // Other providers reuse the Claude normalizer for now (they fail at the
        // application layer before reaching this point); kept for forward
        // compatibility with custom-template overrides.
        HttpBedrockProvider::Other => normalizers::normalize_claude_response(body),
    }
}

/// Reshape an Amazon Nova non-stream `invoke` response into the Claude-style
/// `{ content: [...], stop_reason, usage }` envelope that
/// `normalize_claude_response` already understands. Doing the translation here
/// (rather than building a parallel normalizer) keeps the OpenAI-shape choice
/// the frontend sees identical to the Claude path.
///
/// Nova's response shape (Converse-style even when called via `/invoke`):
/// ```json
/// {
///   "output": { "message": { "role": "assistant", "content": [{ "text": "..." }] } },
///   "stopReason": "end_turn",
///   "usage": { "inputTokens": N, "outputTokens": M, "totalTokens": N+M }
/// }
/// ```
fn nova_response_to_claude_shape(body: Value) -> Value {
    let mut content_blocks: Vec<Value> = Vec::new();
    if let Some(parts) = body
        .pointer("/output/message/content")
        .and_then(Value::as_array)
    {
        for part in parts {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                content_blocks.push(json!({ "type": "text", "text": text }));
            }
        }
    }

    let stop_reason = body
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn")
        .to_string();

    let mut usage = Map::new();
    if let Some(usage_value) = body.get("usage").and_then(Value::as_object) {
        if let Some(input_tokens) = usage_value
            .get("inputTokens")
            .or_else(|| usage_value.get("input_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = usage_value
            .get("outputTokens")
            .or_else(|| usage_value.get("output_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("output_tokens".to_string(), json!(output_tokens));
        }
    }

    let mut claude_body = Map::new();
    claude_body.insert("content".to_string(), Value::Array(content_blocks));
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let stream_endpoint = to_stream_endpoint(endpoint_path)?;
    let url = HttpChatCompletionRepository::build_url(&config.base_url, &stream_endpoint);

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, BEDROCK_EVENTSTREAM_CONTENT_TYPE)
        .json(payload);
    let request = apply_bedrock_auth(request, config);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            BEDROCK_PROVIDER_NAME,
            response,
            "Generation request failed",
        )
        .await);
    }

    let provider = detect_http_provider(extract_model_id_from_endpoint(endpoint_path));
    forward_eventstream_response(response, sender, cancel, provider).await
}

fn apply_bedrock_auth(request: RequestBuilder, config: &ChatCompletionApiConfig) -> RequestBuilder {
    if let Some(authorization_header) = config.authorization_header.as_deref() {
        return HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            authorization_header,
        );
    }

    HttpChatCompletionRepository::apply_bearer_auth(request, &config.api_key)
}

fn validate_invoke_endpoint(endpoint_path: &str) -> Result<(), DomainError> {
    if endpoint_path.ends_with(BEDROCK_INVOKE_SUFFIX) {
        Ok(())
    } else {
        Err(DomainError::InvalidData(format!(
            "AWS Bedrock requires an invoke endpoint path, got {endpoint_path}"
        )))
    }
}

fn to_stream_endpoint(endpoint_path: &str) -> Result<String, DomainError> {
    if let Some(stripped) = endpoint_path.strip_suffix(BEDROCK_INVOKE_SUFFIX) {
        Ok(format!("{stripped}{BEDROCK_STREAM_SUFFIX}"))
    } else if endpoint_path.ends_with(BEDROCK_STREAM_SUFFIX) {
        Ok(endpoint_path.to_string())
    } else {
        Err(DomainError::InvalidData(format!(
            "AWS Bedrock requires an invoke endpoint path, got {endpoint_path}"
        )))
    }
}

async fn forward_eventstream_response(
    mut response: reqwest::Response,
    sender: ChatCompletionStreamSender,
    mut cancel: ChatCompletionCancelReceiver,
    provider: HttpBedrockProvider,
) -> Result<(), DomainError> {
    let mut buffer = Vec::<u8>::new();

    loop {
        if *cancel.borrow() {
            return Ok(());
        }

        let chunk = tokio::select! {
            _ = cancel.changed() => {
                if *cancel.borrow() {
                    return Ok(());
                }
                continue;
            }
            chunk = response.chunk() => {
                chunk.map_err(|error| DomainError::transient(format!(
                    "{BEDROCK_PROVIDER_NAME} stream read failed: {error}"
                )))?
            }
        };

        let Some(chunk) = chunk else {
            break;
        };

        buffer.extend_from_slice(&chunk);
        drain_eventstream_messages(&mut buffer, &sender, provider)?;
    }

    Ok(())
}

fn drain_eventstream_messages(
    buffer: &mut Vec<u8>,
    sender: &ChatCompletionStreamSender,
    provider: HttpBedrockProvider,
) -> Result<(), DomainError> {
    loop {
        match parse_next_message(buffer)? {
            ParseStep::Need => return Ok(()),
            ParseStep::Consumed { consumed, payload } => {
                if !payload.is_empty() {
                    if let Some(forwarded) = decode_eventstream_payload(&payload, provider)? {
                        if sender.send(forwarded).is_err() {
                            buffer.drain(..consumed);
                            return Ok(());
                        }
                    }
                }
                buffer.drain(..consumed);
            }
        }
    }
}

enum ParseStep {
    Need,
    Consumed { consumed: usize, payload: Vec<u8> },
}

fn parse_next_message(buffer: &[u8]) -> Result<ParseStep, DomainError> {
    const PRELUDE_LEN: usize = 12;
    const TRAILER_LEN: usize = 4;

    if buffer.len() < PRELUDE_LEN {
        return Ok(ParseStep::Need);
    }

    let total_length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    let headers_length = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]) as usize;

    if total_length < PRELUDE_LEN + TRAILER_LEN + headers_length {
        return Err(DomainError::InternalError(format!(
            "{BEDROCK_PROVIDER_NAME} stream returned a malformed EventStream frame",
        )));
    }

    if buffer.len() < total_length {
        return Ok(ParseStep::Need);
    }

    let payload_start = PRELUDE_LEN + headers_length;
    let payload_end = total_length - TRAILER_LEN;
    let payload = buffer[payload_start..payload_end].to_vec();

    Ok(ParseStep::Consumed {
        consumed: total_length,
        payload,
    })
}

fn decode_eventstream_payload(
    payload: &[u8],
    provider: HttpBedrockProvider,
) -> Result<Option<String>, DomainError> {
    let value: Value = serde_json::from_slice(payload).map_err(|error| {
        DomainError::InternalError(format!(
            "{BEDROCK_PROVIDER_NAME} stream returned non-JSON EventStream payload: {error}",
        ))
    })?;

    if let Some(bytes_field) = value.get("bytes").and_then(Value::as_str) {
        let decoded = BASE64_STANDARD.decode(bytes_field).map_err(|error| {
            DomainError::InternalError(format!(
                "{BEDROCK_PROVIDER_NAME} stream returned non-base64 bytes payload: {error}",
            ))
        })?;
        let decoded = String::from_utf8(decoded).map_err(|error| {
            DomainError::InternalError(format!(
                "{BEDROCK_PROVIDER_NAME} stream returned non-UTF-8 chunk payload: {error}",
            ))
        })?;
        // Each provider speaks its own chunk dialect on `invoke-with-response-stream`.
        // Normalize to Anthropic-style `content_block_delta` here so the frontend
        // dispatcher in `getStreamingReply` (path: `data.delta.text` /
        // `data.delta.thinking`) works uniformly across every Bedrock provider.
        return Ok(transform_chunk_for_provider(&decoded, provider));
    }

    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Err(DomainError::InternalError(format!(
            "{BEDROCK_PROVIDER_NAME} stream failed: {message}",
        )));
    }

    Ok(None)
}

fn transform_chunk_for_provider(
    decoded: &str,
    provider: HttpBedrockProvider,
) -> Option<String> {
    match provider {
        // Anthropic already emits `{"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}`.
        // Pass through.
        HttpBedrockProvider::Anthropic | HttpBedrockProvider::Other => Some(decoded.to_string()),
        HttpBedrockProvider::Nova => transform_nova_chunk_to_anthropic(decoded),
        HttpBedrockProvider::Llama => transform_llama_chunk_to_anthropic(decoded),
        HttpBedrockProvider::Mistral => transform_mistral_chunk_to_anthropic(decoded),
        HttpBedrockProvider::DeepSeek => transform_deepseek_chunk_to_anthropic(decoded),
        HttpBedrockProvider::Cohere => transform_cohere_chunk_to_anthropic(decoded),
        HttpBedrockProvider::Ai21Jamba => transform_ai21_jamba_chunk_to_anthropic(decoded),
    }
}

/// Llama non-stream response shape (per AWS Bedrock User Guide
/// `model-parameters-meta.md`):
/// ```json
/// { "generation": "...", "prompt_token_count": N, "generation_token_count": M, "stop_reason": "stop" }
/// ```
/// We translate it into a single-block Claude payload so the existing
/// `normalize_claude_response` can fold it into an OpenAI `chat.completion`.
/// Llama's `stop_reason` values (`stop`, `length`) already align with Claude's
/// `end_turn` / `max_tokens` after the Claude finish-reason mapping.
fn llama_response_to_claude_shape(body: Value) -> Value {
    let text = body
        .get("generation")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let stop_reason = match body.get("stop_reason").and_then(Value::as_str) {
        Some("length") => "max_tokens".to_string(),
        Some(other) => other.to_string(),
        None => "end_turn".to_string(),
    };

    let mut usage = Map::new();
    if let Some(input_tokens) = body.get("prompt_token_count").and_then(Value::as_u64) {
        usage.insert("input_tokens".to_string(), json!(input_tokens));
    }
    if let Some(output_tokens) = body.get("generation_token_count").and_then(Value::as_u64) {
        usage.insert("output_tokens".to_string(), json!(output_tokens));
    }

    let mut claude_body = Map::new();
    claude_body.insert(
        "content".to_string(),
        Value::Array(vec![json!({ "type": "text", "text": text })]),
    );
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

/// Llama stream chunks each carry the next token group in `generation`. The
/// terminal chunk also carries `stop_reason` so the frontend never sees
/// `null`. We map every `generation` value (including the empty trailing one)
/// to an Anthropic `content_block_delta` text frame.
fn transform_llama_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = value.get("generation").and_then(Value::as_str)?;
    if text.is_empty() {
        return None;
    }
    Some(
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text },
        })
        .to_string(),
    )
}

/// Mistral on Bedrock exposes two response shapes, switched by model id:
///
/// - **Legacy text-completion** (7B, Mixtral, large-2402): the body is
///   `{ "outputs": [{ "text": "...", "stop_reason": "stop" }] }`.
/// - **Chat-completion** (large-2407+, small, medium, Pixtral): the body may
///   be `{ "content": [{ "role": "assistant", "content": [{ "text": "..." }] }] }`
///   (mistral-large-2407 spec) **or** `{ "choices": [{ "index":0,
///   "message": { "role":"assistant", "content":"string" }, "stop_reason":"stop" }] }`
///   (the generic Mistral chat-completion spec).
///
/// We probe each shape in turn and emit a Claude-style payload so the existing
/// `normalize_claude_response` does the OpenAI-shape rewriting.
fn mistral_response_to_claude_shape(body: Value) -> Value {
    let text = if let Some(outputs) = body.get("outputs").and_then(Value::as_array) {
        outputs
            .iter()
            .filter_map(|output| output.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")
    } else if let Some(choices) = body.get("choices").and_then(Value::as_array) {
        choices
            .iter()
            .filter_map(|choice| {
                choice
                    .pointer("/message/content")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("")
    } else if let Some(content) = body.get("content").and_then(Value::as_array) {
        content
            .iter()
            .filter_map(|message| {
                message
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| {
                                part.get("text").and_then(Value::as_str).map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                            .join("")
                    })
            })
            .collect::<Vec<_>>()
            .join("")
    } else {
        String::new()
    };

    let stop_reason = body
        .pointer("/choices/0/stop_reason")
        .or_else(|| body.pointer("/outputs/0/stop_reason"))
        .and_then(Value::as_str)
        .map(|value| match value {
            "length" => "max_tokens".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "end_turn".to_string());

    let mut usage = Map::new();
    if let Some(usage_value) = body.get("usage").and_then(Value::as_object) {
        if let Some(input_tokens) = usage_value
            .get("prompt_tokens")
            .or_else(|| usage_value.get("input_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = usage_value
            .get("completion_tokens")
            .or_else(|| usage_value.get("output_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("output_tokens".to_string(), json!(output_tokens));
        }
    }

    let mut claude_body = Map::new();
    claude_body.insert(
        "content".to_string(),
        Value::Array(vec![json!({ "type": "text", "text": text })]),
    );
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

/// Mistral stream chunks come in three flavours depending on the model:
///
/// - Legacy text-completion: `{ "outputs": [{ "text": "...", "stop_reason": null }] }`
/// - Chat (large-2407 spec):  `{ "content": [{ "text": "..." }] }`
/// - Chat (chat-completion):  `{ "choices": [{ "delta": { "content": "..." } }] }`
///
/// We probe each one and rewrap the extracted text as an Anthropic
/// `content_block_delta` frame. Chunks without user-visible text (final
/// `stop_reason` markers, empty `content`, ...) are silently dropped.
fn transform_mistral_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;

    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/content/0/text").and_then(Value::as_str))
        .or_else(|| value.pointer("/outputs/0/text").and_then(Value::as_str))?;

    if text.is_empty() {
        return None;
    }

    Some(
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text },
        })
        .to_string(),
    )
}

/// DeepSeek on Bedrock exposes two response shapes, switched by model id:
///
/// - **Text-completion (R1)**: `{ "choices": [{ "text": "...", "stop_reason": "stop"|"length" }] }`.
/// - **Chat-completion (V3.1+ / V3.2)**: AWS model cards only show the
///   request side and pretty-print the response via `json.loads(...)`. The
///   most plausible shape (matching DeepSeek's own OpenAI-compatible API) is
///   `{ "choices": [{ "message": { "role":"assistant","content":"..." }, "finish_reason":"stop" }] }`,
///   but Bedrock could also keep the legacy `text` field. We probe both
///   `choices[0].message.content` and `choices[0].text` defensively.
///
/// The result is rewritten into a Claude-shaped payload so the existing
/// `normalize_claude_response` handles the OpenAI projection.
fn deepseek_response_to_claude_shape(body: Value) -> Value {
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            body.pointer("/choices/0/text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    let stop_reason = body
        .pointer("/choices/0/finish_reason")
        .or_else(|| body.pointer("/choices/0/stop_reason"))
        .and_then(Value::as_str)
        .map(|value| match value {
            "length" => "max_tokens".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "end_turn".to_string());

    let mut usage = Map::new();
    if let Some(usage_value) = body.get("usage").and_then(Value::as_object) {
        if let Some(input_tokens) = usage_value
            .get("prompt_tokens")
            .or_else(|| usage_value.get("input_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = usage_value
            .get("completion_tokens")
            .or_else(|| usage_value.get("output_tokens"))
            .and_then(Value::as_u64)
        {
            usage.insert("output_tokens".to_string(), json!(output_tokens));
        }
    }

    let mut claude_body = Map::new();
    claude_body.insert(
        "content".to_string(),
        Value::Array(vec![json!({ "type": "text", "text": text })]),
    );
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

/// DeepSeek stream chunks (per the published R1 schema and the Bedrock model
/// card examples) come in two flavours:
///
/// - Text-completion (R1): `{ "choices": [{ "text": "...", "stop_reason": null|"stop" }] }`
/// - Chat-completion (V3.1+): `{ "choices": [{ "delta": { "content": "..." } }] }`
///   (OpenAI-compatible — same as DeepSeek's own /v1/chat/completions stream).
///
/// We probe each one and rewrap the extracted text as an Anthropic
/// `content_block_delta`. Terminal frames without user-visible text (empty
/// `text` / empty `delta.content`) are silently dropped so the frontend never
/// re-renders a blank tail.
fn transform_deepseek_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/choices/0/text").and_then(Value::as_str))?;

    if text.is_empty() {
        return None;
    }

    Some(
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text },
        })
        .to_string(),
    )
}

/// AI21 Jamba non-stream responses (per AI21 chat-completion spec /
/// Bedrock model card `model-parameters-jamba.md`):
///
/// ```json
/// { "id": "...", "choices": [{ "index": 0,
///     "message": { "role": "assistant", "content": "..." },
///     "finish_reason": "stop|length|content_filter" }],
///   "usage": { "prompt_tokens": N, "completion_tokens": M } }
/// ```
///
/// We lift `choices[0].message.content` into a Claude-shaped `content[].text`
/// block, project Jamba's `finish_reason` (`length` -> `max_tokens`, `stop` /
/// `content_filter` -> `end_turn` / `content_filter`), and forward
/// `usage.prompt_tokens` / `usage.completion_tokens`.
fn ai21_jamba_response_to_claude_shape(body: Value) -> Value {
    let text = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();

    let stop_reason = body
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .map(|value| match value {
            "length" => "max_tokens".to_string(),
            "stop" => "end_turn".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "end_turn".to_string());

    let mut usage = Map::new();
    if let Some(usage_value) = body.get("usage").and_then(Value::as_object) {
        if let Some(input_tokens) = usage_value.get("prompt_tokens").and_then(Value::as_u64) {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = usage_value
            .get("completion_tokens")
            .and_then(Value::as_u64)
        {
            usage.insert("output_tokens".to_string(), json!(output_tokens));
        }
    }

    let mut claude_body = Map::new();
    claude_body.insert(
        "content".to_string(),
        Value::Array(vec![json!({ "type": "text", "text": text })]),
    );
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

/// AI21 Jamba stream chunks are OpenAI-shape:
///
/// ```json
/// { "id": "...", "choices": [{
///     "index": 0,
///     "delta": { "role": "assistant", "content": "..." },
///     "finish_reason": null | "stop" | "length"
/// }] }
/// ```
///
/// The first chunk carries `delta.role = "assistant"` with no content; the
/// terminal chunk has `delta = {}` (or empty content) and a `usage` summary.
/// We extract `choices[0].delta.content`, drop empty or sentinel chunks, and
/// rewrap as an Anthropic `content_block_delta` text frame.
fn transform_ai21_jamba_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)?;

    if text.is_empty() {
        return None;
    }

    Some(
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text },
        })
        .to_string(),
    )
}

/// Cohere Command R / R+ non-stream responses (per AWS Bedrock User Guide
/// `model-parameters-cohere-command-r-plus.md`) look like:
///
/// ```json
/// { "response_id": "...", "text": "...", "finish_reason": "complete|max_tokens|error|...",
///   "meta": { "billed_units": { "input_tokens": N, "output_tokens": M } } }
/// ```
///
/// We lift `text` straight into a Claude `content[].text` block, project
/// Cohere's `finish_reason` into Claude vocabulary (`complete` -> `end_turn`,
/// `max_tokens` stays, `error_*` -> `error`, ...), and forward
/// `meta.billed_units` as `usage.input_tokens` / `usage.output_tokens`.
fn cohere_response_to_claude_shape(body: Value) -> Value {
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();

    let stop_reason = body
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(map_cohere_finish_reason_to_claude)
        .unwrap_or_else(|| "end_turn".to_string());

    let mut usage = Map::new();
    if let Some(billed) = body
        .pointer("/meta/billed_units")
        .and_then(Value::as_object)
    {
        if let Some(input_tokens) = billed.get("input_tokens").and_then(Value::as_u64) {
            usage.insert("input_tokens".to_string(), json!(input_tokens));
        }
        if let Some(output_tokens) = billed.get("output_tokens").and_then(Value::as_u64) {
            usage.insert("output_tokens".to_string(), json!(output_tokens));
        }
    }

    let mut claude_body = Map::new();
    claude_body.insert(
        "content".to_string(),
        Value::Array(vec![json!({ "type": "text", "text": text })]),
    );
    claude_body.insert("stop_reason".to_string(), Value::String(stop_reason));
    if !usage.is_empty() {
        claude_body.insert("usage".to_string(), Value::Object(usage));
    }
    Value::Object(claude_body)
}

fn map_cohere_finish_reason_to_claude(reason: &str) -> String {
    match reason {
        "complete" => "end_turn".to_string(),
        "max_tokens" => "max_tokens".to_string(),
        // error_toxic / error_limit / error / user_cancel collapse into the
        // generic "error" — Anthropic doesn't enumerate a richer vocabulary.
        other if other.starts_with("error") => "error".to_string(),
        "user_cancel" => "error".to_string(),
        other => other.to_string(),
    }
}

/// Cohere streams chat events keyed by `event_type` (per
/// `bedrock-runtime_example_bedrock-runtime_InvokeModelWithResponseStream_CohereCommandR_section`
/// and the Cohere openapi spec):
///
/// - `{ "event_type": "stream-start",   "generation_id": "..." }`
/// - `{ "event_type": "text-generation","text": "...", "is_finished": false }`
/// - `{ "event_type": "stream-end",     "finish_reason": "...", "response": {...} }`
/// - `{ "event_type": "citation-generation" | "tool-calls-*" | ... }`
///
/// Only `text-generation` carries user-visible text; everything else is
/// dropped. We also accept legacy Cohere Command (`generations[0].text`)
/// chunks defensively so the same path works for `cohere.command-text-v14`
/// stream output if it ever lands here.
fn transform_cohere_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let text = match value.get("event_type").and_then(Value::as_str) {
        Some("text-generation") => value.get("text").and_then(Value::as_str)?,
        Some(_) => return None,
        None => value
            .pointer("/generations/0/text")
            .and_then(Value::as_str)?,
    };

    if text.is_empty() {
        return None;
    }

    Some(
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text },
        })
        .to_string(),
    )
}

/// Nova streams Converse-style events. Each EventStream `bytes` chunk decodes
/// into one of:
///   - `{ "messageStart": { "role": "assistant" } }`
///   - `{ "contentBlockStart": { "start": {...}, "contentBlockIndex": 0 } }`
///   - `{ "contentBlockDelta": { "delta": { "text": "..." }, "contentBlockIndex": 0 } }`
///   - `{ "contentBlockStop": { "contentBlockIndex": 0 } }`
///   - `{ "messageStop": { "stopReason": "end_turn" } }`
///   - `{ "metadata": { "usage": {...} } }`
///
/// Only the `contentBlockDelta.delta.text` payload carries user-visible text;
/// every other event is dropped. The delta is rewrapped as an Anthropic
/// `content_block_delta` text frame.
fn transform_nova_chunk_to_anthropic(decoded: &str) -> Option<String> {
    let value: Value = serde_json::from_str(decoded).ok()?;
    let delta_text = value
        .pointer("/contentBlockDelta/delta/text")
        .and_then(Value::as_str)?;
    let index = value
        .pointer("/contentBlockDelta/contentBlockIndex")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(
        json!({
            "type": "content_block_delta",
            "index": index,
            "delta": { "type": "text_delta", "text": delta_text },
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{
        HttpBedrockProvider, ai21_jamba_response_to_claude_shape, cohere_response_to_claude_shape,
        decode_eventstream_payload, deepseek_response_to_claude_shape,
        derive_control_plane_base, detect_http_provider, drain_eventstream_messages,
        extract_model_id_from_endpoint, extract_provider, inference_supports_on_demand,
        llama_response_to_claude_shape, merge_bedrock_models,
        mistral_response_to_claude_shape, normalize_provider_response,
        nova_response_to_claude_shape, to_stream_endpoint,
        transform_ai21_jamba_chunk_to_anthropic, transform_cohere_chunk_to_anthropic,
        transform_deepseek_chunk_to_anthropic, transform_llama_chunk_to_anthropic,
        transform_mistral_chunk_to_anthropic, transform_nova_chunk_to_anthropic,
        validate_invoke_endpoint,
    };
    use serde_json::Value;

    #[test]
    fn validate_invoke_endpoint_accepts_invoke_suffix() {
        validate_invoke_endpoint("/model/anthropic.claude-sonnet-4-20250514-v1:0/invoke")
            .expect("invoke endpoint should be accepted");
    }

    #[test]
    fn validate_invoke_endpoint_rejects_other_paths() {
        validate_invoke_endpoint("/messages")
            .expect_err("non-invoke endpoint should be rejected");
    }

    #[test]
    fn stream_endpoint_swaps_invoke_for_invoke_with_response_stream() {
        let stream =
            to_stream_endpoint("/model/anthropic.claude-sonnet-4-20250514-v1:0/invoke").unwrap();
        assert_eq!(
            stream,
            "/model/anthropic.claude-sonnet-4-20250514-v1:0/invoke-with-response-stream"
        );
    }

    #[test]
    fn stream_endpoint_is_idempotent() {
        let stream = to_stream_endpoint(
            "/model/anthropic.claude-sonnet-4-20250514-v1:0/invoke-with-response-stream",
        )
        .unwrap();
        assert_eq!(
            stream,
            "/model/anthropic.claude-sonnet-4-20250514-v1:0/invoke-with-response-stream"
        );
    }

    #[test]
    fn decode_eventstream_payload_extracts_inner_chunk_json() {
        let inner = json!({
            "type": "content_block_delta",
            "delta": { "type": "text_delta", "text": "hello" }
        });
        let encoded = BASE64_STANDARD.encode(inner.to_string().as_bytes());
        let payload = json!({ "bytes": encoded }).to_string();
        let decoded = decode_eventstream_payload(payload.as_bytes(), HttpBedrockProvider::Anthropic)
            .unwrap()
            .expect("payload with bytes should decode");
        let parsed: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(parsed["delta"]["text"], "hello");
    }

    #[test]
    fn decode_eventstream_payload_returns_none_for_internal_metadata() {
        let payload = json!({ "p": "ignored" }).to_string();
        let decoded =
            decode_eventstream_payload(payload.as_bytes(), HttpBedrockProvider::Anthropic).unwrap();
        assert!(decoded.is_none(), "metadata payloads should be skipped");
    }

    #[test]
    fn decode_eventstream_payload_surfaces_exception_messages() {
        let payload = json!({ "message": "throttled" }).to_string();
        let error = decode_eventstream_payload(payload.as_bytes(), HttpBedrockProvider::Anthropic)
            .expect_err("exception payload should fail");
        assert!(error.to_string().contains("throttled"));
    }

    #[test]
    fn drain_eventstream_messages_emits_decoded_chunks_in_order() {
        let chunk_one = synthesize_frame(b"first");
        let chunk_two = synthesize_frame(b"second");

        let mut buffer = Vec::new();
        buffer.extend_from_slice(&chunk_one);
        buffer.extend_from_slice(&chunk_two);

        let (sender, mut receiver) = unbounded_channel::<String>();
        drain_eventstream_messages(&mut buffer, &sender, HttpBedrockProvider::Anthropic).unwrap();
        assert!(buffer.is_empty());

        assert_eq!(receiver.try_recv().ok(), Some("first".to_string()));
        assert_eq!(receiver.try_recv().ok(), Some("second".to_string()));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn drain_eventstream_messages_keeps_partial_frame_in_buffer() {
        let chunk = synthesize_frame(b"hello");
        let mut buffer = chunk[..chunk.len() - 1].to_vec();

        let (sender, mut receiver) = unbounded_channel::<String>();
        drain_eventstream_messages(&mut buffer, &sender, HttpBedrockProvider::Anthropic).unwrap();
        assert_eq!(buffer.len(), chunk.len() - 1, "buffer should be retained");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn detect_http_provider_identifies_every_wired_provider() {
        assert_eq!(
            detect_http_provider("us.anthropic.claude-opus-4-7"),
            HttpBedrockProvider::Anthropic,
        );
        assert_eq!(
            detect_http_provider("amazon.nova-pro-v1:0"),
            HttpBedrockProvider::Nova,
        );
        assert_eq!(
            detect_http_provider("meta.llama3-2-3b-instruct-v1:0"),
            HttpBedrockProvider::Llama,
        );
        assert_eq!(
            detect_http_provider("mistral.mistral-large-2407-v1:0"),
            HttpBedrockProvider::Mistral,
        );
        assert_eq!(
            detect_http_provider("us.deepseek.r1-v1:0"),
            HttpBedrockProvider::DeepSeek,
        );
        assert_eq!(
            detect_http_provider("cohere.command-r-plus-v1:0"),
            HttpBedrockProvider::Cohere,
        );
        assert_eq!(
            detect_http_provider("ai21.jamba-1-5-large-v1:0"),
            HttpBedrockProvider::Ai21Jamba,
        );
        assert_eq!(
            detect_http_provider("us.ai21.jamba-instruct-v1:0"),
            HttpBedrockProvider::Ai21Jamba,
        );
        // Legacy Jurassic stays Other (no `.jamba` substring).
        assert_eq!(
            detect_http_provider("ai21.j2-ultra-v1"),
            HttpBedrockProvider::Other,
        );
    }

    #[test]
    fn extract_model_id_from_endpoint_works_for_invoke_and_stream_paths() {
        assert_eq!(
            extract_model_id_from_endpoint("/model/us.amazon.nova-pro-v1:0/invoke"),
            "us.amazon.nova-pro-v1:0",
        );
        assert_eq!(
            extract_model_id_from_endpoint(
                "/model/anthropic.claude-3-haiku-20240307-v1:0/invoke-with-response-stream"
            ),
            "anthropic.claude-3-haiku-20240307-v1:0",
        );
        assert_eq!(extract_model_id_from_endpoint("/chat/completions"), "");
    }

    #[test]
    fn transform_nova_chunk_to_anthropic_extracts_content_block_delta_text() {
        let chunk = json!({
            "contentBlockDelta": {
                "delta": { "text": "hello world" },
                "contentBlockIndex": 0,
            }
        })
        .to_string();
        let rewritten = transform_nova_chunk_to_anthropic(&chunk).expect("delta chunk");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], "hello world");
        assert_eq!(parsed["index"], 0);
    }

    #[test]
    fn transform_nova_chunk_to_anthropic_drops_non_text_envelopes() {
        for chunk in [
            json!({ "messageStart": { "role": "assistant" } }).to_string(),
            json!({ "messageStop": { "stopReason": "end_turn" } }).to_string(),
            json!({ "metadata": { "usage": { "inputTokens": 1, "outputTokens": 1 } } })
                .to_string(),
        ] {
            assert!(
                transform_nova_chunk_to_anthropic(&chunk).is_none(),
                "non-delta chunk should be silently dropped: {chunk}",
            );
        }
    }

    #[test]
    fn nova_response_to_claude_shape_lifts_text_and_usage() {
        let nova_body = json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [
                        { "text": "first" },
                        { "text": "second" }
                    ]
                }
            },
            "stopReason": "end_turn",
            "usage": { "inputTokens": 10, "outputTokens": 5, "totalTokens": 15 }
        });

        let claude_shape = nova_response_to_claude_shape(nova_body);
        assert_eq!(claude_shape["stop_reason"], "end_turn");
        let content = claude_shape["content"].as_array().expect("content");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "first");
        assert_eq!(claude_shape["usage"]["input_tokens"], 10);
        assert_eq!(claude_shape["usage"]["output_tokens"], 5);
    }

    #[test]
    fn transform_llama_chunk_to_anthropic_extracts_generation_text() {
        let chunk = json!({
            "generation": " world",
            "prompt_token_count": 10,
            "generation_token_count": 1,
            "stop_reason": null,
        })
        .to_string();
        let rewritten = transform_llama_chunk_to_anthropic(&chunk).expect("delta chunk");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], " world");
    }

    #[test]
    fn transform_llama_chunk_to_anthropic_drops_trailing_empty_generation() {
        let chunk = json!({ "generation": "", "stop_reason": "stop" }).to_string();
        assert!(transform_llama_chunk_to_anthropic(&chunk).is_none());
    }

    #[test]
    fn llama_response_to_claude_shape_lifts_generation_text_and_token_counts() {
        let body = json!({
            "generation": "hello world",
            "prompt_token_count": 12,
            "generation_token_count": 4,
            "stop_reason": "stop"
        });
        let claude_shape = llama_response_to_claude_shape(body);
        assert_eq!(claude_shape["stop_reason"], "stop");
        let content = claude_shape["content"].as_array().expect("content");
        assert_eq!(content[0]["text"], "hello world");
        assert_eq!(claude_shape["usage"]["input_tokens"], 12);
        assert_eq!(claude_shape["usage"]["output_tokens"], 4);
    }

    #[test]
    fn llama_response_length_stop_reason_maps_to_claude_max_tokens() {
        let body = json!({ "generation": "...", "stop_reason": "length" });
        let claude_shape = llama_response_to_claude_shape(body);
        assert_eq!(claude_shape["stop_reason"], "max_tokens");
    }

    #[test]
    fn transform_mistral_chunk_to_anthropic_handles_all_three_shapes() {
        let legacy = json!({ "outputs": [{ "text": " hi", "stop_reason": null }] }).to_string();
        let chat_2407 = json!({ "content": [{ "text": " from" }] }).to_string();
        let chat_openai = json!({
            "choices": [{ "index": 0, "delta": { "content": " mistral" } }],
        })
        .to_string();

        for (chunk, expected) in [
            (legacy, " hi"),
            (chat_2407, " from"),
            (chat_openai, " mistral"),
        ] {
            let rewritten = transform_mistral_chunk_to_anthropic(&chunk)
                .unwrap_or_else(|| panic!("expected delta from chunk: {chunk}"));
            let parsed: Value = serde_json::from_str(&rewritten).unwrap();
            assert_eq!(parsed["type"], "content_block_delta");
            assert_eq!(parsed["delta"]["text"], expected);
        }
    }

    #[test]
    fn transform_mistral_chunk_to_anthropic_drops_terminal_stop_reason_chunks() {
        let chunk = json!({ "outputs": [{ "text": "", "stop_reason": "stop" }] }).to_string();
        assert!(transform_mistral_chunk_to_anthropic(&chunk).is_none());
    }

    #[test]
    fn mistral_response_to_claude_shape_picks_up_text_from_each_response_dialect() {
        let legacy = json!({
            "outputs": [{ "text": "legacy text", "stop_reason": "stop" }]
        });
        let chat_2407 = json!({
            "content": [{
                "role": "assistant",
                "content": [{ "text": "chat 2407 text" }]
            }]
        });
        let chat_openai = json!({
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "chat openai text" },
                "stop_reason": "length"
            }]
        });

        let legacy_shape = mistral_response_to_claude_shape(legacy);
        assert_eq!(legacy_shape["content"][0]["text"], "legacy text");
        assert_eq!(legacy_shape["stop_reason"], "stop");

        let chat_2407_shape = mistral_response_to_claude_shape(chat_2407);
        assert_eq!(chat_2407_shape["content"][0]["text"], "chat 2407 text");

        let chat_openai_shape = mistral_response_to_claude_shape(chat_openai);
        assert_eq!(chat_openai_shape["content"][0]["text"], "chat openai text");
        assert_eq!(
            chat_openai_shape["stop_reason"], "max_tokens",
            "length must map to Claude max_tokens",
        );
    }

    #[test]
    fn transform_deepseek_chunk_to_anthropic_handles_text_and_delta_shapes() {
        let r1 = json!({
            "choices": [{ "text": " hello", "stop_reason": null }],
        })
        .to_string();
        let v3 = json!({
            "choices": [{ "delta": { "content": " world" } }],
        })
        .to_string();

        for (chunk, expected) in [(r1, " hello"), (v3, " world")] {
            let rewritten = transform_deepseek_chunk_to_anthropic(&chunk)
                .unwrap_or_else(|| panic!("expected delta from chunk: {chunk}"));
            let parsed: Value = serde_json::from_str(&rewritten).unwrap();
            assert_eq!(parsed["type"], "content_block_delta");
            assert_eq!(parsed["delta"]["text"], expected);
        }
    }

    #[test]
    fn transform_deepseek_chunk_to_anthropic_drops_terminal_empty_text() {
        let chunk = json!({
            "choices": [{ "text": "", "stop_reason": "stop" }],
        })
        .to_string();
        assert!(transform_deepseek_chunk_to_anthropic(&chunk).is_none());
    }

    #[test]
    fn deepseek_response_to_claude_shape_lifts_text_and_token_counts() {
        let r1 = json!({
            "choices": [{ "text": "r1 reply", "stop_reason": "length" }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 },
        });
        let r1_shape = deepseek_response_to_claude_shape(r1);
        assert_eq!(r1_shape["content"][0]["text"], "r1 reply");
        assert_eq!(r1_shape["stop_reason"], "max_tokens");
        assert_eq!(r1_shape["usage"]["input_tokens"], 10);
        assert_eq!(r1_shape["usage"]["output_tokens"], 5);

        let v3 = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "v3 reply" },
                "finish_reason": "stop"
            }]
        });
        let v3_shape = deepseek_response_to_claude_shape(v3);
        assert_eq!(v3_shape["content"][0]["text"], "v3 reply");
        assert_eq!(v3_shape["stop_reason"], "stop");
    }

    #[test]
    fn transform_cohere_chunk_to_anthropic_only_keeps_text_generation_events() {
        let text_gen = json!({
            "event_type": "text-generation",
            "text": " hi",
            "is_finished": false,
        })
        .to_string();
        let rewritten = transform_cohere_chunk_to_anthropic(&text_gen)
            .expect("text-generation event must surface a delta");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], " hi");

        let stream_start = json!({
            "event_type": "stream-start",
            "generation_id": "abc",
        })
        .to_string();
        assert!(transform_cohere_chunk_to_anthropic(&stream_start).is_none());

        let stream_end = json!({
            "event_type": "stream-end",
            "finish_reason": "complete",
            "response": {},
        })
        .to_string();
        assert!(transform_cohere_chunk_to_anthropic(&stream_end).is_none());

        // Legacy Cohere Command (generations[0].text) — kept as a defensive
        // fallback for cohere.command-text-v14 / command-light-text-v14 if
        // they ever stream through here.
        let legacy = json!({
            "generations": [{ "text": " legacy" }]
        })
        .to_string();
        let legacy_delta = transform_cohere_chunk_to_anthropic(&legacy)
            .expect("legacy chunks must surface a delta");
        let parsed: Value = serde_json::from_str(&legacy_delta).unwrap();
        assert_eq!(parsed["delta"]["text"], " legacy");
    }

    #[test]
    fn cohere_response_to_claude_shape_maps_finish_reason_and_billed_units() {
        let body = json!({
            "response_id": "abc",
            "text": "cohere reply",
            "finish_reason": "max_tokens",
            "meta": {
                "billed_units": { "input_tokens": 42, "output_tokens": 7 }
            }
        });

        let shape = cohere_response_to_claude_shape(body);
        assert_eq!(shape["content"][0]["text"], "cohere reply");
        assert_eq!(shape["stop_reason"], "max_tokens");
        assert_eq!(shape["usage"]["input_tokens"], 42);
        assert_eq!(shape["usage"]["output_tokens"], 7);

        let complete = json!({ "text": "done", "finish_reason": "complete" });
        assert_eq!(
            cohere_response_to_claude_shape(complete)["stop_reason"],
            "end_turn",
            "complete must map to Claude end_turn",
        );

        let toxic = json!({ "text": "", "finish_reason": "error_toxic" });
        assert_eq!(
            cohere_response_to_claude_shape(toxic)["stop_reason"],
            "error",
            "any error_* must collapse into Claude error",
        );
    }

    #[test]
    fn transform_ai21_jamba_chunk_to_anthropic_extracts_delta_content() {
        let chunk = json!({
            "id": "abc",
            "choices": [{
                "index": 0,
                "delta": { "content": " jamba" },
                "finish_reason": null
            }]
        })
        .to_string();
        let rewritten = transform_ai21_jamba_chunk_to_anthropic(&chunk)
            .expect("jamba delta chunk must surface text");
        let parsed: Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], " jamba");
    }

    #[test]
    fn transform_ai21_jamba_chunk_to_anthropic_drops_role_only_and_terminal_frames() {
        let role_only = json!({
            "choices": [{ "delta": { "role": "assistant" } }],
        })
        .to_string();
        assert!(transform_ai21_jamba_chunk_to_anthropic(&role_only).is_none());

        let terminal = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        assert!(transform_ai21_jamba_chunk_to_anthropic(&terminal).is_none());
    }

    #[test]
    fn ai21_jamba_response_to_claude_shape_maps_finish_reason_and_usage() {
        let body = json!({
            "id": "abc",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "jamba reply" },
                "finish_reason": "length"
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 4 }
        });
        let shape = ai21_jamba_response_to_claude_shape(body);
        assert_eq!(shape["content"][0]["text"], "jamba reply");
        assert_eq!(shape["stop_reason"], "max_tokens");
        assert_eq!(shape["usage"]["input_tokens"], 12);
        assert_eq!(shape["usage"]["output_tokens"], 4);

        let stop = json!({
            "choices": [{ "message": { "content": "ok" }, "finish_reason": "stop" }]
        });
        assert_eq!(
            ai21_jamba_response_to_claude_shape(stop)["stop_reason"],
            "end_turn",
            "stop must map to Claude end_turn",
        );
    }

    #[test]
    fn normalize_provider_response_dispatches_nova_via_claude_normalizer() {
        let nova_body = json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [{ "text": "hi from nova" }]
                }
            },
            "stopReason": "end_turn"
        });

        let normalized =
            normalize_provider_response("/model/us.amazon.nova-pro-v1:0/invoke", nova_body).body;

        assert_eq!(normalized["object"], "chat.completion");
        assert_eq!(normalized["choices"][0]["message"]["content"], "hi from nova");
        assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn derive_control_plane_base_rewrites_runtime_host_to_control_plane_host() {
        assert_eq!(
            derive_control_plane_base("https://bedrock-runtime.us-west-2.amazonaws.com").unwrap(),
            "https://bedrock.us-west-2.amazonaws.com",
        );
        assert_eq!(
            derive_control_plane_base("https://bedrock-runtime.us-east-1.amazonaws.com").unwrap(),
            "https://bedrock.us-east-1.amazonaws.com",
        );
        // Already-control-plane bases pass through (e.g. a reverse-proxy override).
        assert_eq!(
            derive_control_plane_base("https://bedrock.eu-central-1.amazonaws.com").unwrap(),
            "https://bedrock.eu-central-1.amazonaws.com",
        );
        // Trailing slash preserved (build_url already trims it later anyway).
        assert_eq!(
            derive_control_plane_base("https://bedrock-runtime.us-west-2.amazonaws.com/").unwrap(),
            "https://bedrock.us-west-2.amazonaws.com/",
        );
        // Non-Bedrock base cannot be derived; surface a clear error.
        assert!(derive_control_plane_base("https://example.com").is_err());
    }

    #[test]
    fn inference_supports_on_demand_treats_explicit_lists_correctly() {
        let on_demand_only = json!({
            "inferenceTypesSupported": ["ON_DEMAND"]
        });
        assert!(inference_supports_on_demand(&on_demand_only));

        // Claude 4.x foundation models report INFERENCE_PROFILE only.
        let profile_only = json!({
            "inferenceTypesSupported": ["INFERENCE_PROFILE"]
        });
        assert!(!inference_supports_on_demand(&profile_only));

        let mixed = json!({
            "inferenceTypesSupported": ["INFERENCE_PROFILE", "ON_DEMAND"]
        });
        assert!(inference_supports_on_demand(&mixed));

        // Missing/empty list is forward-compatible: assume opt-in.
        let missing = json!({});
        assert!(inference_supports_on_demand(&missing));
        let empty = json!({ "inferenceTypesSupported": [] });
        assert!(inference_supports_on_demand(&empty));
    }

    #[test]
    fn extract_provider_strips_inference_profile_prefix_and_returns_first_segment() {
        assert_eq!(extract_provider("anthropic.claude-3-haiku"), "anthropic");
        assert_eq!(extract_provider("us.anthropic.claude-opus-4-7"), "anthropic");
        assert_eq!(extract_provider("amazon.nova-pro-v1:0"), "amazon");
        assert_eq!(
            extract_provider("us.meta.llama3-3-70b-instruct-v1:0"),
            "meta",
        );
        assert_eq!(extract_provider("mistral.mistral-large-2407-v1:0"), "mistral");
        assert_eq!(extract_provider("cohere.command-r-plus-v1:0"), "cohere");
        assert_eq!(extract_provider("ai21.jamba-1-5-large-v1:0"), "ai21");
        assert_eq!(extract_provider("deepseek.r1-v1:0"), "deepseek");
        assert_eq!(
            extract_provider("global.anthropic.claude-opus-4-6-v1"),
            "anthropic",
        );
    }

    #[test]
    fn merge_bedrock_models_lists_all_providers_and_tags_each_entry() {
        let foundation = json!({
            "modelSummaries": [
                {
                    "modelId": "anthropic.claude-opus-4-7",
                    "modelName": "Claude Opus 4.7",
                    "modelLifecycle": { "status": "ACTIVE" },
                    "inferenceTypesSupported": ["INFERENCE_PROFILE"]
                },
                {
                    "modelId": "anthropic.claude-3-haiku-20240307-v1:0",
                    "modelName": "Claude 3 Haiku",
                    "modelLifecycle": { "status": "ACTIVE" },
                    "inferenceTypesSupported": ["ON_DEMAND"]
                },
                {
                    "modelId": "amazon.titan-text-premier-v1:0",
                    "modelName": "Titan Text Premier",
                    "modelLifecycle": { "status": "ACTIVE" },
                    "inferenceTypesSupported": ["ON_DEMAND"]
                },
                {
                    "modelId": "meta.llama3-2-3b-instruct-v1:0",
                    "modelName": "Llama 3.2 3B Instruct",
                    "modelLifecycle": { "status": "ACTIVE" },
                    "inferenceTypesSupported": ["ON_DEMAND"]
                },
                {
                    "modelId": "anthropic.claude-2",
                    "modelName": "Claude 2",
                    "modelLifecycle": { "status": "LEGACY" }
                }
            ]
        });
        let profiles = json!({
            "inferenceProfileSummaries": [
                {
                    "inferenceProfileId": "us.anthropic.claude-opus-4-7",
                    "inferenceProfileName": "US Claude Opus 4.7",
                    "status": "ACTIVE"
                },
                {
                    "inferenceProfileId": "us.meta.llama3-3-70b-instruct-v1:0",
                    "inferenceProfileName": "US Llama 3.3 70B Instruct",
                    "status": "ACTIVE"
                },
                {
                    "inferenceProfileId": "us.amazon.nova-pro-v1:0",
                    "inferenceProfileName": "US Nova Pro",
                    "status": "ACTIVE"
                },
                {
                    "inferenceProfileId": "us.anthropic.claude-archived",
                    "inferenceProfileName": "Archived",
                    "status": "INACTIVE"
                }
            ]
        });

        let merged = merge_bedrock_models(&foundation, &profiles);
        let by_id: std::collections::HashMap<&str, &serde_json::Value> = merged
            .iter()
            .filter_map(|item| {
                item.get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(|id| (id, item))
            })
            .collect();

        // ON_DEMAND foundation models from every provider are kept.
        assert!(by_id.contains_key("anthropic.claude-3-haiku-20240307-v1:0"));
        assert!(by_id.contains_key("amazon.titan-text-premier-v1:0"));
        assert!(by_id.contains_key("meta.llama3-2-3b-instruct-v1:0"));
        // INFERENCE_PROFILE-only foundation entries are hidden (their
        // cross-region profile variants surface from /inference-profiles).
        assert!(!by_id.contains_key("anthropic.claude-opus-4-7"));
        // LEGACY models are dropped.
        assert!(!by_id.contains_key("anthropic.claude-2"));
        // ACTIVE inference profiles for *any* provider are kept now (no more
        // Anthropic-only filter); the UI is responsible for marking unsupported
        // providers.
        assert!(by_id.contains_key("us.anthropic.claude-opus-4-7"));
        assert!(by_id.contains_key("us.meta.llama3-3-70b-instruct-v1:0"));
        assert!(by_id.contains_key("us.amazon.nova-pro-v1:0"));
        // Non-ACTIVE profiles are dropped.
        assert!(!by_id.contains_key("us.anthropic.claude-archived"));

        // Each entry carries its origin (foundation-model vs inference-profile)
        // and an extracted `provider` so the frontend can group/tag.
        let nova = by_id["us.amazon.nova-pro-v1:0"];
        assert_eq!(
            nova.get("source").and_then(serde_json::Value::as_str),
            Some("inference-profile")
        );
        assert_eq!(
            nova.get("provider").and_then(serde_json::Value::as_str),
            Some("amazon")
        );

        let llama_foundation = by_id["meta.llama3-2-3b-instruct-v1:0"];
        assert_eq!(
            llama_foundation
                .get("source")
                .and_then(serde_json::Value::as_str),
            Some("foundation-model")
        );
        assert_eq!(
            llama_foundation
                .get("provider")
                .and_then(serde_json::Value::as_str),
            Some("meta")
        );
    }

    /// Build a synthetic EventStream frame whose payload is `{ "bytes": base64(text) }`.
    /// Headers are omitted (headers_length = 0). CRCs are written as zero placeholders
    /// because parsing logic intentionally does not validate them.
    fn synthesize_frame(text: &[u8]) -> Vec<u8> {
        let encoded = BASE64_STANDARD.encode(text);
        let payload = format!("{{\"bytes\":\"{encoded}\"}}");
        let payload_bytes = payload.as_bytes();

        let prelude_len: usize = 12;
        let trailer_len: usize = 4;
        let headers_len: usize = 0;
        let total_len = (prelude_len + headers_len + payload_bytes.len() + trailer_len) as u32;

        let mut frame = Vec::with_capacity(total_len as usize);
        frame.extend_from_slice(&total_len.to_be_bytes());
        frame.extend_from_slice(&(headers_len as u32).to_be_bytes());
        frame.extend_from_slice(&[0_u8; 4]);
        frame.extend_from_slice(payload_bytes);
        frame.extend_from_slice(&[0_u8; 4]);
        frame
    }
}
