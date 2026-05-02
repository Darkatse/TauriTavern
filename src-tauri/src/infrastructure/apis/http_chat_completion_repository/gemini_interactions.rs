use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;

const GEMINI_API_VERSION: &str = "v1beta";

struct InteractionsStreamState {
    created: u64,
    model: String,
    interaction_id: Option<String>,
    sent_role: bool,
    saw_tool_call: bool,
    done_sent: bool,
    outputs_by_index: HashMap<usize, Value>,
}

impl InteractionsStreamState {
    fn new(model: String) -> Self {
        Self {
            created: current_unix_timestamp(),
            model,
            interaction_id: None,
            sent_role: false,
            saw_tool_call: false,
            done_sent: false,
            outputs_by_index: HashMap::new(),
        }
    }

    fn handle_event(&mut self, sender: &ChatCompletionStreamSender, raw_payload: &[u8]) {
        if self.done_sent {
            return;
        }

        let Ok(event) = serde_json::from_slice::<Value>(raw_payload) else {
            return;
        };

        let Some(event_object) = event.as_object() else {
            return;
        };

        let event_type = event_object
            .get("event_type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "interaction.start" => {
                if let Some(id) = event_object
                    .get("interaction")
                    .and_then(Value::as_object)
                    .and_then(|interaction| interaction.get("id"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    self.interaction_id = Some(id.to_string());
                }
            }
            "content.start" => self.apply_content_start(event_object),
            "content.delta" => self.apply_content_delta(event_object, sender),
            "interaction.complete" => self.apply_interaction_complete(event_object, sender),
            "error" => self.apply_error(event_object, sender),
            _ => {}
        }
    }

    fn apply_content_start(&mut self, event: &serde_json::Map<String, Value>) {
        let index = event
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize;

        let Some(content) = event.get("content").and_then(Value::as_object) else {
            return;
        };

        if content
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|ty| ty == "function_call")
        {
            self.saw_tool_call = true;
        }

        self.outputs_by_index
            .insert(index, Value::Object(content.clone()));
    }

    fn apply_content_delta(
        &mut self,
        event: &serde_json::Map<String, Value>,
        sender: &ChatCompletionStreamSender,
    ) {
        let index = event
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize;

        let Some(delta) = event.get("delta").and_then(Value::as_object) else {
            return;
        };

        let delta_type = delta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let output_entry = self
            .outputs_by_index
            .entry(index)
            .or_insert_with(|| json!({}));

        let Some(output) = output_entry.as_object_mut() else {
            return;
        };

        match delta_type {
            "text" => {
                let text = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if text.is_empty() {
                    return;
                }

                append_string_field(output, "text", text);
                self.send_delta(sender, json!({ "content": text }), None);
            }
            "thought_summary" => {
                let summary = delta
                    .get("content")
                    .and_then(Value::as_object)
                    .and_then(|content| content.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if summary.is_empty() {
                    return;
                }

                output.insert("type".to_string(), Value::String("thought".to_string()));
                append_string_field(output, "summary", summary);
                self.send_delta(sender, json!({ "reasoning_content": summary }), None);
            }
            "thought_signature" => {
                let signature = delta
                    .get("signature")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default();
                if signature.is_empty() {
                    return;
                }

                output.insert("type".to_string(), Value::String("thought".to_string()));
                output.insert(
                    "signature".to_string(),
                    Value::String(signature.to_string()),
                );
            }
            "function_call" => {
                self.saw_tool_call = true;
                output.insert(
                    "type".to_string(),
                    Value::String("function_call".to_string()),
                );

                let id = delta
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("tool_call");
                let name = delta
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("tool");
                let arguments = delta
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

                output.insert("id".to_string(), Value::String(id.to_string()));
                output.insert("name".to_string(), Value::String(name.to_string()));
                output.insert("arguments".to_string(), arguments.clone());

                if let Some(signature) = delta
                    .get("signature")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    output.insert(
                        "signature".to_string(),
                        Value::String(signature.to_string()),
                    );
                }

                let arguments =
                    serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".to_string());

                let mut tool_call = json!({
                    "index": index,
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                });

                if let Some(signature) = output.get("signature").and_then(Value::as_str) {
                    if let Some(object) = tool_call.as_object_mut() {
                        object.insert(
                            "signature".to_string(),
                            Value::String(signature.to_string()),
                        );
                    }
                }

                self.send_delta(
                    sender,
                    json!({
                        "tool_calls": [tool_call]
                    }),
                    None,
                );
            }
            _ => {
                for (key, value) in delta {
                    if key == "type" {
                        continue;
                    }
                    output.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn apply_interaction_complete(
        &mut self,
        event: &serde_json::Map<String, Value>,
        sender: &ChatCompletionStreamSender,
    ) {
        if let Some(id) = event
            .get("interaction")
            .and_then(Value::as_object)
            .and_then(|interaction| interaction.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.interaction_id = Some(id.to_string());
        }

        let finish_reason = if self.saw_tool_call {
            "tool_calls"
        } else {
            "stop"
        };

        let outputs = self.finalize_outputs();
        let native = json!({
            "gemini_interactions": {
                "outputs": outputs,
            }
        });

        self.send_delta(sender, json!({ "native": native }), Some(finish_reason));
        let _ = sender.send("[DONE]".to_string());
        self.done_sent = true;
    }

    fn apply_error(
        &mut self,
        event: &serde_json::Map<String, Value>,
        sender: &ChatCompletionStreamSender,
    ) {
        let message = event
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Gemini Interactions stream failed");

        let _ = sender.send(
            serde_json::to_string(&json!({ "error": { "message": message } })).unwrap_or_default(),
        );
        let _ = sender.send("[DONE]".to_string());
        self.done_sent = true;
    }

    fn finalize_outputs(&self) -> Vec<Value> {
        let mut indices = self.outputs_by_index.keys().cloned().collect::<Vec<_>>();
        indices.sort_unstable();

        indices
            .into_iter()
            .filter_map(|index| self.outputs_by_index.get(&index).cloned())
            .collect()
    }

    fn send_delta(
        &mut self,
        sender: &ChatCompletionStreamSender,
        delta: Value,
        finish_reason: Option<&str>,
    ) {
        if !self.sent_role {
            self.sent_role = true;
            let role_chunk = self.build_chunk(json!({ "role": "assistant" }), None);
            if let Ok(payload) = serde_json::to_string(&role_chunk) {
                let _ = sender.send(payload);
            }
        }

        let chunk = self.build_chunk(delta, finish_reason);
        if let Ok(payload) = serde_json::to_string(&chunk) {
            let _ = sender.send(payload);
        }
    }

    fn build_chunk(&self, delta: Value, finish_reason: Option<&str>) -> Value {
        let id = self
            .interaction_id
            .clone()
            .unwrap_or_else(|| "gemini-interactions-stream".to_string());

        json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason
            }]
        })
    }
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let url = build_gemini_url(&config.base_url, endpoint_path);

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(payload);

    let request = apply_gemini_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
            response,
            "Generation request failed",
        )
        .await);
    }

    let body = response.json::<Value>().await.map_err(|error| {
        DomainError::InternalError(format!("Failed to parse generation JSON: {error}"))
    })?;

    Ok(normalizers::normalize_gemini_interactions_response(body))
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
    let url = build_gemini_url(&config.base_url, endpoint_path);

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(payload);

    let request = apply_gemini_stream_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);

    let response = request.send().await.map_err(|error| {
        DomainError::InternalError(format!("Generation request failed: {error}"))
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
            response,
            "Generation request failed",
        )
        .await);
    }

    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut state = InteractionsStreamState::new(model);
    let out_sender = sender.clone();

    let (dummy_sender, dummy_receiver) = mpsc::unbounded_channel::<String>();
    drop(dummy_receiver);

    HttpChatCompletionRepository::stream_sse_response_internal(
        provider_name,
        response,
        dummy_sender,
        cancel,
        move |payload| {
            state.handle_event(&out_sender, payload);
        },
    )
    .await
}

fn apply_gemini_auth(
    request: reqwest::RequestBuilder,
    config: &ChatCompletionApiConfig,
) -> reqwest::RequestBuilder {
    if let Some(authorization_header) = config.authorization_header.as_deref() {
        return HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            authorization_header,
        );
    }

    let request = HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-goog-api-key",
        &config.api_key,
    );

    if config.api_key.trim().is_empty() {
        request
    } else {
        request.query(&[("key", config.api_key.as_str())])
    }
}

fn apply_gemini_stream_auth(
    request: reqwest::RequestBuilder,
    config: &ChatCompletionApiConfig,
) -> reqwest::RequestBuilder {
    if let Some(authorization_header) = config.authorization_header.as_deref() {
        let request = HttpChatCompletionRepository::apply_header_if_present(
            request,
            "Authorization",
            authorization_header,
        );
        return request.query(&[("alt", "sse")]);
    }

    let request = HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-goog-api-key",
        &config.api_key,
    );

    if config.api_key.trim().is_empty() {
        request.query(&[("alt", "sse")])
    } else {
        request.query(&[("key", config.api_key.as_str()), ("alt", "sse")])
    }
}

fn build_gemini_url(base_url: &str, endpoint_path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let suffix = endpoint_path.trim().trim_start_matches('/');

    if trimmed.ends_with("/v1") || trimmed.ends_with("/v1beta") {
        format!("{trimmed}/{suffix}")
    } else {
        format!("{trimmed}/{GEMINI_API_VERSION}/{suffix}")
    }
}

fn append_string_field(target: &mut serde_json::Map<String, Value>, key: &str, delta: &str) {
    let existing = target.get(key).and_then(Value::as_str).unwrap_or_default();
    let mut combined = String::new();
    combined.push_str(existing);
    combined.push_str(delta);
    target.insert(key.to_string(), Value::String(combined));
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
