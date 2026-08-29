use std::collections::HashMap;

use reqwest::RequestBuilder;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{Map, Value};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::{
    AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
    ChatCompletionToolCallDelta,
};

use super::HttpChatCompletionRepository;
use super::normalizers;
use super::response_body::read_upstream_json_body;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_BETA_OUTPUT_128K: &str = "output-128k-2025-02-19";
const ANTHROPIC_BETA_CONTEXT_1M: &str = "context-1m-2025-08-07";
const ANTHROPIC_BETA_PROMPT_CACHING: &str = "prompt-caching-2024-07-31";
const ANTHROPIC_BETA_EXTENDED_CACHE_TTL: &str = "extended-cache-ttl-2025-04-11";

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, "/models")?;

    let client = repository.metadata_client(config)?;
    let request = client
        .get(url)
        .header(ACCEPT, "application/json")
        .header("anthropic-version", ANTHROPIC_VERSION);

    let request = apply_claude_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Status request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            "Claude",
            response,
            "Failed to list models",
        )
        .await);
    }

    read_upstream_json_body("Claude", "list_models", response).await
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let endpoint_path = if endpoint_path.trim().is_empty() {
        "/messages"
    } else {
        endpoint_path
    };

    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path)?;

    let client = repository.client(config)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(payload);

    let request = apply_claude_auth(request, config);
    let request = apply_configured_anthropic_beta_headers(request, config, payload);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = read_upstream_json_body(provider_name, "generate", response).await?;

    if super::payload_contains_cache_control(payload) {
        let model = payload.get("model").and_then(Value::as_str);
        let _ = super::log_prompt_cache_performance_if_present(provider_name, model, &body);
    }

    Ok(normalizers::normalize_claude_response(body))
}

pub(super) async fn generate_stream(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let response =
        send_stream_request(repository, config, endpoint_path, payload, provider_name).await?;

    if super::payload_contains_cache_control(payload) {
        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut logged = false;

        HttpChatCompletionRepository::stream_sse_response_internal(
            provider_name,
            response,
            sender,
            cancel,
            move |payload| {
                if logged {
                    return Ok(());
                }

                if !payload
                    .windows(b"cache_read_input_tokens".len())
                    .any(|window| window == b"cache_read_input_tokens")
                    && !payload
                        .windows(b"cache_creation_input_tokens".len())
                        .any(|window| window == b"cache_creation_input_tokens")
                {
                    return Ok(());
                }

                let Ok(value) = serde_json::from_slice::<Value>(payload) else {
                    return Ok(());
                };

                logged = super::log_prompt_cache_performance_if_present(
                    provider_name,
                    Some(model.as_str()),
                    &value,
                );
                Ok(())
            },
        )
        .await
    } else {
        HttpChatCompletionRepository::stream_sse_response(provider_name, response, sender, cancel)
            .await
    }
}

pub(super) async fn generate_with_tool_call_deltas(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
    on_tool_call_delta: &mut (dyn FnMut(ChatCompletionToolCallDelta) + Send),
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let response =
        send_stream_request(repository, config, endpoint_path, payload, provider_name).await?;
    let body = consume_message_stream(provider_name, response, on_tool_call_delta).await?;

    if super::payload_contains_cache_control(payload) {
        let model = payload.get("model").and_then(Value::as_str);
        let _ = super::log_prompt_cache_performance_if_present(provider_name, model, &body);
    }

    Ok(normalizers::normalize_claude_response(body))
}

pub(super) async fn consume_message_stream(
    provider_name: &str,
    response: reqwest::Response,
    on_tool_call_delta: &mut (dyn FnMut(ChatCompletionToolCallDelta) + Send),
) -> Result<Value, DomainError> {
    let mut accumulator = ClaudeMessageAccumulator::default();
    let mut completed = None;

    HttpChatCompletionRepository::consume_sse_response(provider_name, response, |event| {
        if let Some(message) = accumulator.apply_event(event, on_tool_call_delta)? {
            completed = Some(message);
        }
        Ok(())
    })
    .await?;

    require_message_stop(completed)
}

async fn send_stream_request(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
) -> Result<reqwest::Response, DomainError> {
    let endpoint_path = if endpoint_path.trim().is_empty() {
        "/messages"
    } else {
        endpoint_path
    };
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path)?;
    let client = repository.stream_client(config)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(payload);
    let request = apply_claude_auth(request, config);
    let request = apply_configured_anthropic_beta_headers(request, config, payload);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Generation request failed", error)
    })?;
    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
            response,
            "Generation request failed",
        )
        .await);
    }

    Ok(response)
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClaudeStreamEvent {
    MessageStart {
        message: Map<String, Value>,
    },
    ContentBlockStart {
        index: usize,
        content_block: Value,
    },
    ContentBlockDelta {
        index: usize,
        delta: ClaudeContentDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: Map<String, Value>,
        #[serde(default)]
        usage: Map<String, Value>,
    },
    MessageStop,
    Ping,
    Error {
        error: Value,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::enum_variant_names,
    reason = "variants mirror Claude wire event names"
)]
enum ClaudeContentDelta {
    TextDelta { text: String },
    ThinkingDelta { thinking: String },
    SignatureDelta { signature: String },
    CitationsDelta { citation: Value },
    InputJsonDelta { partial_json: String },
}

#[derive(Default)]
pub(super) struct ClaudeMessageAccumulator {
    message: Option<Map<String, Value>>,
    input_json: String,
}

impl ClaudeMessageAccumulator {
    pub(super) fn apply_event(
        &mut self,
        raw_event: &[u8],
        on_tool_call_delta: &mut dyn FnMut(ChatCompletionToolCallDelta),
    ) -> Result<Option<Value>, DomainError> {
        let event: ClaudeStreamEvent = serde_json::from_slice(raw_event)
            .map_err(|error| invalid_claude_stream(format!("event is invalid: {error}")))?;

        match event {
            ClaudeStreamEvent::MessageStart { message } => {
                self.message = Some(message);
            }
            ClaudeStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let content = self.content_mut()?;
                if index != content.len() {
                    return Err(invalid_claude_stream("content block index is out of order"));
                }
                content.push(content_block);
            }
            ClaudeStreamEvent::ContentBlockDelta { index, delta } => {
                self.apply_content_delta(index, delta, on_tool_call_delta)?;
            }
            ClaudeStreamEvent::ContentBlockStop { index } => {
                let input_json = std::mem::take(&mut self.input_json);
                if !input_json.is_empty() {
                    let input = serde_json::from_str(&input_json).map_err(|error| {
                        invalid_claude_stream(format!("tool input is not valid JSON: {error}"))
                    })?;
                    self.block_mut(index)?
                        .as_object_mut()
                        .ok_or_else(|| invalid_claude_stream("content block must be an object"))?
                        .insert("input".to_string(), input);
                }
            }
            ClaudeStreamEvent::MessageDelta { delta, usage } => {
                let message = self.message_mut()?;
                message.extend(delta);
                if !usage.is_empty() {
                    message
                        .entry("usage".to_string())
                        .or_insert_with(|| Value::Object(Map::new()))
                        .as_object_mut()
                        .ok_or_else(|| invalid_claude_stream("usage must be an object"))?
                        .extend(usage);
                }
            }
            ClaudeStreamEvent::MessageStop => {
                if !self.input_json.is_empty() {
                    return Err(invalid_claude_stream("tool input is incomplete"));
                }
                let message = self
                    .message
                    .take()
                    .ok_or_else(|| invalid_claude_stream("message_stop arrived before message"))?;
                return Ok(Some(Value::Object(message)));
            }
            ClaudeStreamEvent::Error { error } => {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| error.to_string());
                return Err(invalid_claude_stream(message));
            }
            ClaudeStreamEvent::Ping => {}
        }

        Ok(None)
    }

    fn apply_content_delta(
        &mut self,
        index: usize,
        delta: ClaudeContentDelta,
        on_tool_call_delta: &mut dyn FnMut(ChatCompletionToolCallDelta),
    ) -> Result<(), DomainError> {
        match delta {
            ClaudeContentDelta::TextDelta { text } => {
                append_block_string(self.block_mut(index)?, "text", &text)
            }
            ClaudeContentDelta::ThinkingDelta { thinking } => {
                append_block_string(self.block_mut(index)?, "thinking", &thinking)
            }
            ClaudeContentDelta::SignatureDelta { signature } => {
                append_block_string(self.block_mut(index)?, "signature", &signature)
            }
            ClaudeContentDelta::CitationsDelta { citation } => {
                let block = self
                    .block_mut(index)?
                    .as_object_mut()
                    .ok_or_else(|| invalid_claude_stream("content block must be an object"))?;
                let citations = block
                    .entry("citations".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if citations.is_null() {
                    *citations = Value::Array(Vec::new());
                }
                citations
                    .as_array_mut()
                    .ok_or_else(|| invalid_claude_stream("citations must be an array"))?
                    .push(citation);
                Ok(())
            }
            ClaudeContentDelta::InputJsonDelta { partial_json } => {
                self.input_json.push_str(&partial_json);
                let content = self.content_mut()?;
                let block = content
                    .get(index)
                    .ok_or_else(|| invalid_claude_stream("content block does not exist"))?;
                if block.get("type").and_then(Value::as_str) == Some("tool_use")
                    && !partial_json.is_empty()
                    && let Some(name) = block.get("name").and_then(Value::as_str)
                {
                    let tool_call_index = content[..index]
                        .iter()
                        .filter(|block| {
                            block.get("type").and_then(Value::as_str) == Some("tool_use")
                        })
                        .count();
                    on_tool_call_delta(ChatCompletionToolCallDelta {
                        tool_call_index,
                        name: name.to_string(),
                        arguments_fragment: partial_json,
                    });
                }
                Ok(())
            }
        }
    }

    fn message_mut(&mut self) -> Result<&mut Map<String, Value>, DomainError> {
        self.message
            .as_mut()
            .ok_or_else(|| invalid_claude_stream("event arrived before message_start"))
    }

    fn content_mut(&mut self) -> Result<&mut Vec<Value>, DomainError> {
        self.message_mut()?
            .get_mut("content")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| invalid_claude_stream("message content must be an array"))
    }

    fn block_mut(&mut self, index: usize) -> Result<&mut Value, DomainError> {
        self.content_mut()?
            .get_mut(index)
            .ok_or_else(|| invalid_claude_stream("content block does not exist"))
    }
}

pub(super) fn require_message_stop(message: Option<Value>) -> Result<Value, DomainError> {
    message.ok_or_else(|| invalid_claude_stream("ended before message_stop"))
}

fn append_block_string(block: &mut Value, field: &str, fragment: &str) -> Result<(), DomainError> {
    let target = block
        .as_object_mut()
        .ok_or_else(|| invalid_claude_stream("content block must be an object"))?
        .entry(field.to_string())
        .or_insert_with(|| Value::String(String::new()));
    match target {
        Value::String(target) => {
            target.push_str(fragment);
            Ok(())
        }
        _ => Err(invalid_claude_stream(
            "content block field must be a string",
        )),
    }
}

fn invalid_claude_stream(message: impl std::fmt::Display) -> DomainError {
    DomainError::transient(format!(
        "model.upstream_invalid_response: Claude Messages stream {message}"
    ))
}

fn apply_claude_auth(request: RequestBuilder, config: &ChatCompletionApiConfig) -> RequestBuilder {
    if let Some(authorization_header) = config.authorization_header.as_deref() {
        return HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            authorization_header,
        );
    }

    HttpChatCompletionRepository::apply_header_if_present(request, "x-api-key", &config.api_key)
}

fn apply_configured_anthropic_beta_headers(
    request: RequestBuilder,
    config: &ChatCompletionApiConfig,
    payload: &Value,
) -> RequestBuilder {
    let beta_values = build_anthropic_beta_values(
        &config.extra_headers,
        payload,
        config.anthropic_beta_header_mode,
    );

    if beta_values.is_empty() {
        return HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    }

    let request = request.header("anthropic-beta", beta_values.join(","));
    HttpChatCompletionRepository::apply_extra_headers_with_filter(
        request,
        &config.extra_headers,
        |key, _| key.eq_ignore_ascii_case("anthropic-beta"),
    )
}

fn build_anthropic_beta_values(
    extra_headers: &HashMap<String, String>,
    payload: &Value,
    mode: AnthropicBetaHeaderMode,
) -> Vec<String> {
    let mut beta_values = match mode {
        AnthropicBetaHeaderMode::None => Vec::new(),
        AnthropicBetaHeaderMode::PromptCachingOnly => Vec::new(),
        AnthropicBetaHeaderMode::ClaudeDefaults => vec![
            ANTHROPIC_BETA_OUTPUT_128K.to_string(),
            ANTHROPIC_BETA_CONTEXT_1M.to_string(),
        ],
    };

    for value in configured_anthropic_beta_values(extra_headers) {
        if !beta_values.iter().any(|existing| existing == &value) {
            beta_values.push(value);
        }
    }

    if super::payload_contains_cache_control(payload) {
        for value in [
            ANTHROPIC_BETA_PROMPT_CACHING,
            ANTHROPIC_BETA_EXTENDED_CACHE_TTL,
        ] {
            if !beta_values.iter().any(|existing| existing == value) {
                beta_values.push(value.to_string());
            }
        }
    }

    beta_values
}

fn configured_anthropic_beta_values(extra_headers: &HashMap<String, String>) -> Vec<String> {
    let Some(raw_value) = extra_headers
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case("anthropic-beta").then_some(value))
    else {
        return Vec::new();
    };

    raw_value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{
        ANTHROPIC_BETA_CONTEXT_1M, ANTHROPIC_BETA_EXTENDED_CACHE_TTL, ANTHROPIC_BETA_OUTPUT_128K,
        ANTHROPIC_BETA_PROMPT_CACHING, ClaudeMessageAccumulator, build_anthropic_beta_values,
        configured_anthropic_beta_values, require_message_stop,
    };
    use tt_ports::repositories::chat_completion_repository::{
        AnthropicBetaHeaderMode, ChatCompletionToolCallDelta,
    };

    #[test]
    fn claude_stream_projects_tool_input_and_builds_agent_final() {
        let events = [
            br#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.as_slice(),
            br#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.as_slice(),
            br#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#.as_slice(),
            br#"{"type":"content_block_stop","index":0}"#.as_slice(),
            br#"{"type":"content_block_start","index":1,"content_block":{"type":"server_tool_use","id":"srv_1","name":"web_search","input":{}}}"#.as_slice(),
            br#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"rust\"}"}}"#.as_slice(),
            br#"{"type":"content_block_stop","index":1}"#.as_slice(),
            br#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"call_1","name":"workspace_write_file","input":{}}}"#.as_slice(),
            br#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a.md\",\"content\":\"hel"}}"#.as_slice(),
            br#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"lo\"}"}}"#.as_slice(),
            br#"{"type":"content_block_stop","index":2}"#.as_slice(),
            br#"{"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":8}}"#.as_slice(),
            br#"{"type":"message_stop"}"#.as_slice(),
        ];
        let mut accumulator = ClaudeMessageAccumulator::default();
        let mut completed = None;
        let mut deltas = Vec::<ChatCompletionToolCallDelta>::new();

        for event in events {
            if let Some(message) = accumulator
                .apply_event(event, &mut |delta| deltas.push(delta))
                .unwrap()
            {
                completed = Some(message);
            }
        }

        assert_eq!(
            deltas,
            vec![
                ChatCompletionToolCallDelta {
                    tool_call_index: 0,
                    name: "workspace_write_file".to_string(),
                    arguments_fragment: "{\"path\":\"a.md\",\"content\":\"hel".to_string(),
                },
                ChatCompletionToolCallDelta {
                    tool_call_index: 0,
                    name: "workspace_write_file".to_string(),
                    arguments_fragment: "lo\"}".to_string(),
                },
            ]
        );

        let message = require_message_stop(completed).unwrap();
        assert_eq!(message["usage"]["input_tokens"], 10);
        assert_eq!(message["usage"]["output_tokens"], 8);
        assert_eq!(message["content"][1]["input"], json!({ "query": "rust" }));

        let body = super::normalizers::normalize_claude_response(message).body;
        assert_eq!(body["choices"][0]["message"]["content"], "hello");
        assert_eq!(
            body["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"content\":\"hello\",\"path\":\"a.md\"}"
        );
    }

    #[test]
    fn detects_cache_control_recursively() {
        let payload = json!({
            "messages": [{
                "content": [{
                    "type": "text",
                    "cache_control": { "type": "ephemeral", "ttl": "5m" }
                }]
            }]
        });

        assert!(super::super::payload_contains_cache_control(&payload));
    }

    #[test]
    fn parses_existing_beta_header_values() {
        let mut headers = HashMap::new();
        headers.insert(
            "anthropic-beta".to_string(),
            format!(
                "  {}, {}  ",
                ANTHROPIC_BETA_PROMPT_CACHING, ANTHROPIC_BETA_EXTENDED_CACHE_TTL
            ),
        );

        let parsed = configured_anthropic_beta_values(&headers);
        assert_eq!(
            parsed,
            vec![
                ANTHROPIC_BETA_PROMPT_CACHING.to_string(),
                ANTHROPIC_BETA_EXTENDED_CACHE_TTL.to_string()
            ]
        );
    }

    #[test]
    fn always_includes_default_beta_values() {
        let headers = HashMap::new();
        let payload = json!({ "messages": [{"role": "user", "content": "hello"}] });

        let beta_values = build_anthropic_beta_values(
            &headers,
            &payload,
            AnthropicBetaHeaderMode::ClaudeDefaults,
        );
        assert!(beta_values.contains(&ANTHROPIC_BETA_OUTPUT_128K.to_string()));
        assert!(beta_values.contains(&ANTHROPIC_BETA_CONTEXT_1M.to_string()));
    }

    #[test]
    fn cache_control_adds_cache_beta_values() {
        let headers = HashMap::new();
        let payload = json!({
            "messages": [{
                "content": [{
                    "type": "text",
                    "cache_control": { "type": "ephemeral", "ttl": "5m" }
                }]
            }]
        });

        let beta_values = build_anthropic_beta_values(
            &headers,
            &payload,
            AnthropicBetaHeaderMode::ClaudeDefaults,
        );
        assert!(beta_values.contains(&ANTHROPIC_BETA_PROMPT_CACHING.to_string()));
        assert!(beta_values.contains(&ANTHROPIC_BETA_EXTENDED_CACHE_TTL.to_string()));
    }

    #[test]
    fn prompt_caching_only_mode_omits_non_caching_beta_values() {
        let headers = HashMap::new();
        let payload = json!({
            "messages": [{
                "content": [{
                    "type": "text",
                    "cache_control": { "type": "ephemeral", "ttl": "5m" }
                }]
            }]
        });

        let beta_values = build_anthropic_beta_values(
            &headers,
            &payload,
            AnthropicBetaHeaderMode::PromptCachingOnly,
        );
        assert!(!beta_values.contains(&ANTHROPIC_BETA_OUTPUT_128K.to_string()));
        assert!(!beta_values.contains(&ANTHROPIC_BETA_CONTEXT_1M.to_string()));
        assert!(beta_values.contains(&ANTHROPIC_BETA_PROMPT_CACHING.to_string()));
        assert!(beta_values.contains(&ANTHROPIC_BETA_EXTENDED_CACHE_TTL.to_string()));
    }
}
