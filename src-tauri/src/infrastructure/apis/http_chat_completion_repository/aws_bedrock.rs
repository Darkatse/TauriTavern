//! AWS Bedrock infrastructure adapter.
//!
//! Orchestrates HTTP calls to Bedrock's `/model/{id}/invoke` and
//! `/invoke-with-response-stream` endpoints plus the regional control plane
//! catalog endpoints (`/foundation-models`, `/inference-profiles`).
//!
//! Each provider's request body construction lives in
//! `application::services::chat_completion_service::payload::aws_bedrock::*`;
//! their response / stream-chunk projections live in the sibling submodules
//! (`anthropic`, `nova`, `llama`, `mistral`, `deepseek`, `cohere`,
//! `ai21_jamba`). Everything here is the routing glue: pick the right
//! provider, run the HTTP request, drain the EventStream, and forward
//! normalized text frames upstream.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_domain::models::bedrock_model::{BedrockModelFamily, BedrockModelSpec, extract_provider};
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;
use super::response_body::read_upstream_json_body;

mod ai21_jamba;
mod cohere;
mod custom;
mod deepseek;
mod llama;
mod mistral;
mod nova;

const BEDROCK_PROVIDER_NAME: &str = "AWS Bedrock";
const BEDROCK_EVENTSTREAM_CONTENT_TYPE: &str = "application/vnd.amazon.eventstream";
const BEDROCK_INVOKE_SUFFIX: &str = "/invoke";
const BEDROCK_STREAM_SUFFIX: &str = "/invoke-with-response-stream";
const BEDROCK_RUNTIME_HOST_INFIX: &str = "bedrock-runtime.";
const BEDROCK_CONTROL_PLANE_HOST_INFIX: &str = "bedrock.";

fn extract_model_id_from_endpoint(endpoint_path: &str) -> Result<&str, DomainError> {
    let Some(rest) = endpoint_path.strip_prefix("/model/") else {
        return Err(DomainError::InvalidData(format!(
            "AWS Bedrock endpoint path must start with /model/, got {endpoint_path}"
        )));
    };
    let Some((model, _)) = rest.rsplit_once('/') else {
        return Err(DomainError::InvalidData(format!(
            "AWS Bedrock endpoint path is missing an invoke suffix: {endpoint_path}"
        )));
    };
    let model = model.trim();
    if model.is_empty() {
        return Err(DomainError::InvalidData(
            "AWS Bedrock endpoint path is missing a model id".to_string(),
        ));
    }
    Ok(model)
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
    let foundation_url = format!("{control_plane_base}/foundation-models?byOutputModality=TEXT");
    let profiles_url = format!("{control_plane_base}/inference-profiles");

    let client = repository.client()?;
    // Doing the two calls in sequence (rather than `tokio::try_join!`) keeps
    // the dependency graph small and matters very little here: each call is a
    // small JSON GET against the regional control plane.
    let foundation =
        get_control_plane_json(&client, config, &foundation_url, "foundation-models").await?;
    let profiles =
        get_control_plane_json(&client, config, &profiles_url, "inference-profiles").await?;

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
    arr.iter().any(|value| value.as_str() == Some("ON_DEMAND"))
}

fn tauritavern_bedrock_metadata(id: &str) -> Value {
    let spec = BedrockModelSpec::classify(id);
    json!({
        "bedrock": {
            "provider": spec.provider(),
            "normalizedId": spec.normalized_id(),
            "family": spec.family_key(),
            "supported": spec.is_supported(),
            "unsupportedReason": spec.unsupported_reason(),
            "capabilities": spec.capabilities(),
        }
    })
}

fn merge_bedrock_models(foundation: &Value, profiles: &Value) -> Vec<Value> {
    let mut entries: Vec<Value> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    if let Some(items) = foundation.get("modelSummaries").and_then(Value::as_array) {
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
                "tauritavern": tauritavern_bedrock_metadata(id),
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
                "tauritavern": tauritavern_bedrock_metadata(id),
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
    let response_mode = response_mode_from_endpoint(
        endpoint_path,
        config.aws_bedrock_custom_response_path.as_deref(),
    )?;
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
    normalize_provider_response(body, response_mode)
}

fn normalize_provider_response(
    body: Value,
    mode: ResponseMode,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let response = match mode {
        ResponseMode::Custom(path) => {
            normalizers::normalize_claude_response(custom::response_to_claude_shape(body, &path)?)
        }
        ResponseMode::Family(BedrockModelFamily::AnthropicClaude) => {
            normalizers::normalize_claude_response(body)
        }
        ResponseMode::Family(BedrockModelFamily::AmazonNova) => {
            normalizers::normalize_claude_response(nova::response_to_claude_shape(body))
        }
        ResponseMode::Family(BedrockModelFamily::MetaLlama) => {
            normalizers::normalize_claude_response(llama::response_to_claude_shape(body))
        }
        ResponseMode::Family(
            BedrockModelFamily::MistralTextCompletion | BedrockModelFamily::MistralChat,
        ) => normalizers::normalize_claude_response(mistral::response_to_claude_shape(body)),
        ResponseMode::Family(
            BedrockModelFamily::DeepSeekTextCompletion | BedrockModelFamily::DeepSeekChat,
        ) => normalizers::normalize_claude_response(deepseek::response_to_claude_shape(body)),
        ResponseMode::Family(BedrockModelFamily::CohereCommandR) => {
            normalizers::normalize_claude_response(cohere::response_to_claude_shape(body))
        }
        ResponseMode::Family(BedrockModelFamily::Ai21Jamba) => {
            normalizers::normalize_claude_response(ai21_jamba::response_to_claude_shape(body))
        }
        ResponseMode::Family(BedrockModelFamily::Unsupported) => {
            return Err(DomainError::InvalidData(
                "AWS Bedrock unsupported model reached response normalization without custom template paths".to_string(),
            ));
        }
    };
    Ok(response)
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
    let stream_mode = stream_mode_from_endpoint(
        endpoint_path,
        config.aws_bedrock_custom_stream_path.as_deref(),
    )?;
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

    forward_eventstream_response(response, sender, cancel, stream_mode).await
}

#[derive(Debug, Clone)]
enum ResponseMode {
    Family(BedrockModelFamily),
    Custom(String),
}

/// Stream-side dispatch mode used by `forward_eventstream_response`. Keeps
/// the per-frame transform decision out of the hot path.
#[derive(Debug, Clone)]
enum StreamMode {
    Family(BedrockModelFamily),
    Custom(String),
}

fn response_mode_from_endpoint(
    endpoint_path: &str,
    custom_response_path: Option<&str>,
) -> Result<ResponseMode, DomainError> {
    if let Some(path) = custom_response_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(ResponseMode::Custom(path.to_string()));
    }
    let spec = classify_endpoint_model(endpoint_path)?;
    require_supported_family(&spec)?;
    Ok(ResponseMode::Family(spec.family()))
}

fn stream_mode_from_endpoint(
    endpoint_path: &str,
    custom_stream_path: Option<&str>,
) -> Result<StreamMode, DomainError> {
    if let Some(path) = custom_stream_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(StreamMode::Custom(path.to_string()));
    }
    let spec = classify_endpoint_model(endpoint_path)?;
    require_supported_family(&spec)?;
    Ok(StreamMode::Family(spec.family()))
}

fn classify_endpoint_model(endpoint_path: &str) -> Result<BedrockModelSpec, DomainError> {
    Ok(BedrockModelSpec::classify(extract_model_id_from_endpoint(
        endpoint_path,
    )?))
}

fn require_supported_family(spec: &BedrockModelSpec) -> Result<(), DomainError> {
    if spec.is_supported() {
        return Ok(());
    }
    let reason = spec
        .unsupported_reason()
        .unwrap_or("This Bedrock model family is not wired by TauriTavern's built-in adapter yet.");
    Err(DomainError::InvalidData(format!(
        "AWS Bedrock model `{}` is not supported by TauriTavern's built-in Bedrock adapter. {reason} Enable the custom template (`aws_bedrock_use_custom_template`) with response paths, or choose a supported Bedrock family.",
        spec.raw_id()
    )))
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
    mode: StreamMode,
) -> Result<(), DomainError> {
    let mut buffer = Vec::<u8>::new();
    let endpoint = response.url().clone();

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
                chunk.map_err(|error| {
                    let failure = crate::infrastructure::http_error::reqwest_body_failure(
                        &error,
                        Some(&endpoint),
                    );
                    tracing::warn!(
                        provider = BEDROCK_PROVIDER_NAME,
                        operation = "eventstream",
                        code = %failure.code,
                        category = %failure.category,
                        endpoint = failure.endpoint.as_deref().unwrap_or(""),
                        timeout = error.is_timeout(),
                        connect = error.is_connect(),
                        body = error.is_body(),
                        request = error.is_request(),
                        "upstream event stream read failed",
                    );
                    DomainError::upstream_failure(failure)
                })?
            }
        };

        let Some(chunk) = chunk else {
            break;
        };

        buffer.extend_from_slice(&chunk);
        drain_eventstream_messages(&mut buffer, &sender, &mode)?;
    }

    Ok(())
}

fn drain_eventstream_messages(
    buffer: &mut Vec<u8>,
    sender: &ChatCompletionStreamSender,
    mode: &StreamMode,
) -> Result<(), DomainError> {
    loop {
        match parse_next_message(buffer)? {
            ParseStep::Need => return Ok(()),
            ParseStep::Consumed { consumed, payload } => {
                if !payload.is_empty() {
                    if let Some(forwarded) = decode_eventstream_payload(&payload, mode)? {
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
    mode: &StreamMode,
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
        // Custom-template streams take precedence: the user-supplied JSON
        // path replaces the per-provider extraction logic.
        return transform_chunk_for_mode(&decoded, mode);
    }

    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Err(DomainError::InternalError(format!(
            "{BEDROCK_PROVIDER_NAME} stream failed: {message}",
        )));
    }

    Ok(None)
}

fn transform_chunk_for_mode(
    decoded: &str,
    mode: &StreamMode,
) -> Result<Option<String>, DomainError> {
    match mode {
        StreamMode::Custom(path) => custom::transform_chunk_to_anthropic(decoded, path),
        StreamMode::Family(family) => Ok(transform_chunk_for_family(decoded, *family)),
    }
}

fn transform_chunk_for_family(decoded: &str, family: BedrockModelFamily) -> Option<String> {
    match family {
        // Anthropic already emits `{"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}`.
        // Pass through.
        BedrockModelFamily::AnthropicClaude => Some(decoded.to_string()),
        BedrockModelFamily::AmazonNova => nova::transform_chunk_to_anthropic(decoded),
        BedrockModelFamily::MetaLlama => llama::transform_chunk_to_anthropic(decoded),
        BedrockModelFamily::MistralTextCompletion | BedrockModelFamily::MistralChat => {
            mistral::transform_chunk_to_anthropic(decoded)
        }
        BedrockModelFamily::DeepSeekTextCompletion | BedrockModelFamily::DeepSeekChat => {
            deepseek::transform_chunk_to_anthropic(decoded)
        }
        BedrockModelFamily::CohereCommandR => cohere::transform_chunk_to_anthropic(decoded),
        BedrockModelFamily::Ai21Jamba => ai21_jamba::transform_chunk_to_anthropic(decoded),
        BedrockModelFamily::Unsupported => None,
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;

    use tt_domain::models::bedrock_model::{BedrockModelFamily, extract_provider};

    use super::{
        ResponseMode, StreamMode, decode_eventstream_payload, derive_control_plane_base,
        drain_eventstream_messages, extract_model_id_from_endpoint, inference_supports_on_demand,
        merge_bedrock_models, normalize_provider_response, response_mode_from_endpoint,
        to_stream_endpoint, validate_invoke_endpoint,
    };

    #[test]
    fn validate_invoke_endpoint_accepts_invoke_suffix() {
        validate_invoke_endpoint("/model/anthropic.claude-sonnet-4-20250514-v1:0/invoke")
            .expect("invoke endpoint should be accepted");
    }

    #[test]
    fn validate_invoke_endpoint_rejects_other_paths() {
        validate_invoke_endpoint("/messages").expect_err("non-invoke endpoint should be rejected");
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
        let decoded = decode_eventstream_payload(
            payload.as_bytes(),
            &StreamMode::Family(BedrockModelFamily::AnthropicClaude),
        )
        .unwrap()
        .expect("payload with bytes should decode");
        let parsed: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(parsed["delta"]["text"], "hello");
    }

    #[test]
    fn decode_eventstream_payload_returns_none_for_internal_metadata() {
        let payload = json!({ "p": "ignored" }).to_string();
        let decoded = decode_eventstream_payload(
            payload.as_bytes(),
            &StreamMode::Family(BedrockModelFamily::AnthropicClaude),
        )
        .unwrap();
        assert!(decoded.is_none(), "metadata payloads should be skipped");
    }

    #[test]
    fn decode_eventstream_payload_surfaces_exception_messages() {
        let payload = json!({ "message": "throttled" }).to_string();
        let error = decode_eventstream_payload(
            payload.as_bytes(),
            &StreamMode::Family(BedrockModelFamily::AnthropicClaude),
        )
        .expect_err("exception payload should fail");
        assert!(error.to_string().contains("throttled"));
    }

    #[test]
    fn decode_eventstream_payload_custom_mode_uses_user_supplied_path() {
        let inner = json!({ "delta": { "text": "custom-chunk" } });
        let encoded = BASE64_STANDARD.encode(inner.to_string().as_bytes());
        let payload = json!({ "bytes": encoded }).to_string();
        let decoded = decode_eventstream_payload(
            payload.as_bytes(),
            &StreamMode::Custom("delta.text".to_string()),
        )
        .unwrap()
        .expect("custom path must surface a delta");
        let parsed: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(parsed["type"], "content_block_delta");
        assert_eq!(parsed["delta"]["text"], "custom-chunk");
    }

    #[test]
    fn drain_eventstream_messages_emits_decoded_chunks_in_order() {
        let chunk_one = synthesize_frame(b"first");
        let chunk_two = synthesize_frame(b"second");

        let mut buffer = Vec::new();
        buffer.extend_from_slice(&chunk_one);
        buffer.extend_from_slice(&chunk_two);

        let (sender, mut receiver) = unbounded_channel::<String>();
        drain_eventstream_messages(
            &mut buffer,
            &sender,
            &StreamMode::Family(BedrockModelFamily::AnthropicClaude),
        )
        .unwrap();
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
        drain_eventstream_messages(
            &mut buffer,
            &sender,
            &StreamMode::Family(BedrockModelFamily::AnthropicClaude),
        )
        .unwrap();
        assert_eq!(buffer.len(), chunk.len() - 1, "buffer should be retained");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn extract_model_id_from_endpoint_works_for_invoke_and_stream_paths() {
        assert_eq!(
            extract_model_id_from_endpoint("/model/us.amazon.nova-pro-v1:0/invoke").unwrap(),
            "us.amazon.nova-pro-v1:0",
        );
        assert_eq!(
            extract_model_id_from_endpoint(
                "/model/anthropic.claude-3-haiku-20240307-v1:0/invoke-with-response-stream"
            )
            .unwrap(),
            "anthropic.claude-3-haiku-20240307-v1:0",
        );
        assert!(extract_model_id_from_endpoint("/chat/completions").is_err());
    }

    #[test]
    fn response_mode_rejects_unsupported_builtin_model_without_custom_path() {
        let error =
            response_mode_from_endpoint("/model/amazon.titan-text-premier-v1:0/invoke", None)
                .expect_err("unsupported family must fail before normalization");

        assert!(error.to_string().contains("not supported"));
        assert!(error.to_string().contains("custom template"));
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

        let normalized = normalize_provider_response(
            nova_body,
            ResponseMode::Family(BedrockModelFamily::AmazonNova),
        )
        .expect("nova response should normalize")
        .body;

        assert_eq!(normalized["object"], "chat.completion");
        assert_eq!(
            normalized["choices"][0]["message"]["content"],
            "hi from nova"
        );
        assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn normalize_provider_response_honours_custom_response_path_over_auto_dispatch() {
        // Even though the model id looks like Nova, the custom-template path
        // should bypass provider-specific normalizers entirely. We point the
        // path at an arbitrary nested string and verify that's what surfaces.
        let body = json!({
            "anything": {
                "user_defined": [
                    { "value": "custom path wins" }
                ]
            }
        });

        let normalized = normalize_provider_response(
            body,
            ResponseMode::Custom("anything.user_defined.0.value".to_string()),
        )
        .expect("custom response should normalize")
        .body;

        assert_eq!(normalized["object"], "chat.completion");
        assert_eq!(
            normalized["choices"][0]["message"]["content"],
            "custom path wins",
        );
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
        assert_eq!(
            extract_provider("us.anthropic.claude-opus-4-7"),
            "anthropic"
        );
        assert_eq!(extract_provider("amazon.nova-pro-v1:0"), "amazon");
        assert_eq!(
            extract_provider("us.meta.llama3-3-70b-instruct-v1:0"),
            "meta",
        );
        assert_eq!(
            extract_provider("mistral.mistral-large-2407-v1:0"),
            "mistral"
        );
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
        // ACTIVE inference profiles for any provider are kept; TauriTavern
        // metadata marks support status for the UI.
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
        assert_eq!(
            nova.pointer("/tauritavern/bedrock/family")
                .and_then(serde_json::Value::as_str),
            Some("amazon_nova")
        );
        assert_eq!(
            nova.pointer("/tauritavern/bedrock/supported")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            nova.pointer("/tauritavern/bedrock/capabilities/stream")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );

        let titan = by_id["amazon.titan-text-premier-v1:0"];
        assert_eq!(
            titan
                .pointer("/tauritavern/bedrock/family")
                .and_then(serde_json::Value::as_str),
            Some("unsupported")
        );
        assert_eq!(
            titan
                .pointer("/tauritavern/bedrock/supported")
                .and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert!(
            titan
                .pointer("/tauritavern/bedrock/unsupportedReason")
                .and_then(serde_json::Value::as_str)
                .is_some()
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
