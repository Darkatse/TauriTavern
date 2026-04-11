use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;

#[derive(Debug, Clone)]
struct ToolCallDescriptor {
    call_id: String,
    name: Option<String>,
}

struct ResponsesStreamState {
    created: u64,
    model: String,
    response_id: Option<String>,
    sent_role: bool,
    saw_tool_call: bool,
    done_sent: bool,
    tool_call_by_item_id: HashMap<String, ToolCallDescriptor>,
    tool_call_by_output_index: HashMap<usize, String>,
}

impl ResponsesStreamState {
    fn new(model: String) -> Self {
        Self {
            created: current_unix_timestamp(),
            model,
            response_id: None,
            sent_role: false,
            saw_tool_call: false,
            done_sent: false,
            tool_call_by_item_id: HashMap::new(),
            tool_call_by_output_index: HashMap::new(),
        }
    }

    fn handle_event(
        &mut self,
        repository: &HttpChatCompletionRepository,
        base_url: &str,
        sender: &ChatCompletionStreamSender,
        raw_payload: &[u8],
    ) {
        if self.done_sent {
            return;
        }

        let Ok(event) = serde_json::from_slice::<Value>(raw_payload) else {
            return;
        };

        if let Some(event_type) = event.get("type").and_then(Value::as_str) {
            match event_type {
                "response.output_text.delta" | "response.text.delta" => {
                    if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            self.send_delta(sender, json!({ "content": delta }), None);
                        }
                    }
                }
                "response.reasoning_text.delta"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning.delta" => {
                    if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            self.send_delta(sender, json!({ "reasoning_content": delta }), None);
                        }
                    }
                }
                "response.output_item.added" => {
                    let Some(item) = event.get("item").and_then(Value::as_object) else {
                        return;
                    };

                    if item.get("type").and_then(Value::as_str) != Some("function_call") {
                        return;
                    }

                    let response_id = event
                        .get("response_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    let call_id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    let item_id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty());

                    let (Some(response_id), Some(call_id), Some(item_id)) =
                        (response_id, call_id, item_id)
                    else {
                        return;
                    };

                    self.response_id = Some(response_id.to_string());
                    self.saw_tool_call = true;

                    let output_index = event
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;

                    self.tool_call_by_item_id.insert(
                        item_id.to_string(),
                        ToolCallDescriptor {
                            call_id: call_id.to_string(),
                            name: name.map(str::to_string),
                        },
                    );
                    self.tool_call_by_output_index
                        .insert(output_index, call_id.to_string());

                    remember_previous_response_id(repository, base_url, call_id, response_id);

                    self.send_delta(
                        sender,
                        json!({
                            "tool_calls": [{
                                "index": output_index,
                                "id": call_id,
                                "type": "function",
                                "function": {
                                    "name": name.unwrap_or("tool"),
                                    "arguments": ""
                                }
                            }]
                        }),
                        None,
                    );
                }
                "response.function_call_arguments.delta" => {
                    let delta = event
                        .get("delta")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if delta.is_empty() {
                        return;
                    }

                    let output_index = event
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;

                    let call_id = event
                        .get("item_id")
                        .and_then(Value::as_str)
                        .and_then(|item_id| self.tool_call_by_item_id.get(item_id))
                        .map(|descriptor| descriptor.call_id.as_str())
                        .or_else(|| {
                            self.tool_call_by_output_index
                                .get(&output_index)
                                .map(|value| value.as_str())
                        })
                        .unwrap_or_default();

                    if call_id.is_empty() {
                        return;
                    }

                    self.send_delta(
                        sender,
                        json!({
                            "tool_calls": [{
                                "index": output_index,
                                "id": call_id,
                                "type": "function",
                                "function": { "arguments": delta }
                            }]
                        }),
                        None,
                    );
                }
                "response.function_call_arguments.done" => {
                    let output_index = event
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;

                    let item_id = event.get("item_id").and_then(Value::as_str);
                    let call_id = item_id
                        .and_then(|id| self.tool_call_by_item_id.get(id))
                        .map(|descriptor| descriptor.call_id.as_str())
                        .or_else(|| {
                            event
                                .get("call_id")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                        })
                        .unwrap_or_default();

                    if call_id.is_empty() {
                        return;
                    }

                    let name = item_id
                        .and_then(|id| self.tool_call_by_item_id.get(id))
                        .and_then(|descriptor| descriptor.name.as_deref())
                        .or_else(|| event.get("name").and_then(Value::as_str))
                        .unwrap_or("tool");

                    let arguments = event
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default();

                    if !arguments.is_empty() {
                        self.send_delta(
                            sender,
                            json!({
                                "tool_calls": [{
                                    "index": output_index,
                                    "id": call_id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": arguments
                                    }
                                }]
                            }),
                            None,
                        );
                    }
                }
                "response.completed" | "response.done" | "response.incomplete" => {
                    let finish_reason = if self.saw_tool_call {
                        "tool_calls"
                    } else {
                        "stop"
                    };

                    self.send_delta(sender, json!({}), Some(finish_reason));
                    let _ = sender.send("[DONE]".to_string());
                    self.done_sent = true;
                }
                "response.failed" => {
                    let message = event
                        .get("response")
                        .and_then(|response| response.get("error"))
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("OpenAI Responses stream failed");

                    let _ = sender.send(
                        serde_json::to_string(&json!({ "error": { "message": message } }))
                            .unwrap_or_default(),
                    );
                    let _ = sender.send("[DONE]".to_string());
                    self.done_sent = true;
                }
                "error" => {
                    let message = event
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("OpenAI Responses stream failed");

                    let _ = sender.send(
                        serde_json::to_string(&json!({ "error": { "message": message } }))
                            .unwrap_or_default(),
                    );
                    let _ = sender.send("[DONE]".to_string());
                    self.done_sent = true;
                }
                _ => {}
            }
        }
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
            .response_id
            .clone()
            .unwrap_or_else(|| "openai-responses-stream".to_string());

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
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);
    let payload = apply_tool_followup_payload(repository, &config.base_url, payload)?;

    let client = repository.client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&payload);

    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
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

    remember_response_call_ids(repository, &config.base_url, &body)?;

    Ok(normalizers::normalize_openai_responses_response(body))
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
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);
    let payload = apply_tool_followup_payload(repository, &config.base_url, payload)?;

    let client = repository.stream_client()?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(&payload);

    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
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
    let base_url = config.base_url.clone();
    let mut state = ResponsesStreamState::new(model);
    let out_sender = sender.clone();
    let repository_ref = repository;

    let (dummy_sender, dummy_receiver) = mpsc::unbounded_channel::<String>();
    drop(dummy_receiver);

    HttpChatCompletionRepository::stream_sse_response_internal(
        provider_name,
        response,
        dummy_sender,
        cancel,
        move |payload| {
            state.handle_event(repository_ref, &base_url, &out_sender, payload);
        },
    )
    .await
}

fn apply_tool_followup_payload(
    repository: &HttpChatCompletionRepository,
    base_url: &str,
    payload: &Value,
) -> Result<Value, DomainError> {
    let Value::Object(object) = payload else {
        return Ok(payload.clone());
    };

    let Some(input) = object.get("input").and_then(Value::as_array) else {
        return Ok(payload.clone());
    };

    let last_assistant_index = input.iter().rposition(|item| {
        item.get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|role| role.eq_ignore_ascii_case("assistant"))
    });

    let tail_start = last_assistant_index.map(|index| index + 1).unwrap_or(0);
    let tail = input.get(tail_start..).unwrap_or_default();

    let call_ids = tail
        .iter()
        .filter_map(|item| {
            let ty = item.get("type").and_then(Value::as_str)?;
            if ty != "function_call_output" {
                return None;
            }

            item.get("call_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();

    if call_ids.is_empty() {
        return Ok(payload.clone());
    }

    let map = repository
        .openai_responses_previous_response_id_by_call_id
        .lock()
        .map_err(|_| {
            DomainError::InternalError(
                "OpenAI Responses previous_response_id cache lock poisoned".to_string(),
            )
        })?;

    let mut previous_response_id = None::<String>;
    for call_id in &call_ids {
        let key = format!("{}::{}", base_url.trim_end_matches('/'), call_id);
        let resolved = map.get(&key).cloned().ok_or_else(|| {
            DomainError::InvalidData(format!(
                "OpenAI Responses tool follow-up is missing previous_response_id for call_id={call_id}. Try regenerating the tool call."
            ))
        })?;

        match &previous_response_id {
            None => previous_response_id = Some(resolved),
            Some(existing) => {
                if existing != &resolved {
                    return Err(DomainError::InvalidData(
                        "OpenAI Responses tool follow-up contains call_ids from multiple responses. Try regenerating the tool call.".to_string(),
                    ));
                }
            }
        }
    }

    drop(map);

    let Some(previous_response_id) = previous_response_id else {
        return Ok(payload.clone());
    };

    let mut adjusted = object.clone();
    adjusted.insert(
        "previous_response_id".to_string(),
        Value::String(previous_response_id),
    );
    adjusted.insert("input".to_string(), Value::Array(tail.to_vec()));

    Ok(Value::Object(adjusted))
}

fn remember_response_call_ids(
    repository: &HttpChatCompletionRepository,
    base_url: &str,
    response: &Value,
) -> Result<(), DomainError> {
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            DomainError::InternalError("OpenAI Responses response is missing id".to_string())
        })?;

    let output = response
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for item in output {
        let Some(object) = item.as_object() else {
            continue;
        };

        if object.get("type").and_then(Value::as_str) != Some("function_call") {
            continue;
        }

        let Some(call_id) = object.get("call_id").and_then(Value::as_str) else {
            continue;
        };

        remember_previous_response_id(repository, base_url, call_id, response_id);
    }

    Ok(())
}

fn remember_previous_response_id(
    repository: &HttpChatCompletionRepository,
    base_url: &str,
    call_id: &str,
    response_id: &str,
) {
    let Ok(mut map) = repository
        .openai_responses_previous_response_id_by_call_id
        .lock()
    else {
        return;
    };

    map.insert(
        format!("{}::{}", base_url.trim_end_matches('/'), call_id),
        response_id.to_string(),
    );
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
