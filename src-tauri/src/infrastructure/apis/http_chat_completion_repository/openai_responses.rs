use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request as WsRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION as WS_AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;

type ResponsesWsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Default)]
pub(super) struct ResponsesWsSessionPool {
    sessions: Mutex<HashMap<String, Arc<Mutex<ResponsesWsSession>>>>,
}

struct ResponsesWsSession {
    connection_key: String,
    socket: ResponsesWsStream,
}

impl ResponsesWsSessionPool {
    async fn session(
        &self,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        session_id: &str,
    ) -> Result<Arc<Mutex<ResponsesWsSession>>, DomainError> {
        let connection_key = ws_connection_key(config, endpoint_path)?;
        if let Some(session) = self.sessions.lock().await.get(session_id).cloned() {
            if session.lock().await.connection_key == connection_key {
                return Ok(session);
            }
        }

        let socket = connect_responses_ws(config, endpoint_path).await?;
        let session = Arc::new(Mutex::new(ResponsesWsSession {
            connection_key,
            socket,
        }));
        self.sessions
            .lock()
            .await
            .insert(session_id.to_string(), session.clone());
        Ok(session)
    }

    pub(super) async fn close(&self, session_id: &str) {
        let session = self.sessions.lock().await.remove(session_id);
        if let Some(session) = session {
            let close_result = session.lock().await.close().await;
            if let Err(error) = close_result {
                tracing::warn!(
                    session_id,
                    error = %error,
                    "Failed to close OpenAI Responses WebSocket session"
                );
            }
        }
    }
}

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

    fn handle_event(&mut self, sender: &ChatCompletionStreamSender, raw_payload: &[u8]) {
        if self.done_sent {
            return;
        }

        let Ok(event) = serde_json::from_slice::<Value>(raw_payload) else {
            return;
        };

        if let Some(response_id) = event
            .get("response_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.response_id = Some(response_id.to_string());
        }

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

    fn has_emitted(&self) -> bool {
        self.sent_role || self.done_sent
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
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    if let Some(session_id) = provider_session_id(payload)? {
        return generate_persistent_ws(
            &repository.openai_responses_ws_sessions,
            config,
            endpoint_path,
            payload,
            &session_id,
        )
        .await;
    }

    match generate_ws(config, endpoint_path, payload).await {
        Ok(response) => Ok(response),
        Err(error) if is_cancelled(&error) => Err(error),
        Err(error) => {
            tracing::warn!(
                provider = provider_name,
                error = %error,
                "OpenAI Responses WebSocket transport failed; falling back to HTTP"
            );
            generate_http(repository, config, endpoint_path, payload, provider_name).await
        }
    }
}

async fn generate_http(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let client = repository.client()?;
    let http_payload = upstream_payload(payload)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(&http_payload);

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
    match generate_stream_ws(
        config,
        endpoint_path,
        payload,
        sender.clone(),
        cancel.clone(),
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(error) if is_cancelled(&error.error) => Err(error.error),
        Err(error) if !error.emitted => {
            tracing::warn!(
                provider = provider_name,
                error = %error.error,
                "OpenAI Responses WebSocket stream failed before output; falling back to HTTP"
            );
            generate_stream_http(
                repository,
                config,
                endpoint_path,
                payload,
                provider_name,
                sender,
                cancel,
            )
            .await
        }
        Err(error) => Err(error.error),
    }
}

async fn generate_stream_http(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
    sender: ChatCompletionStreamSender,
    cancel: ChatCompletionCancelReceiver,
) -> Result<(), DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path);

    let client = repository.stream_client()?;
    let http_payload = upstream_payload(payload)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(&http_payload);

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
    let mut state = ResponsesStreamState::new(model);
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

struct ResponsesWsStreamError {
    error: DomainError,
    emitted: bool,
}

async fn generate_persistent_ws(
    pool: &ResponsesWsSessionPool,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    session_id: &str,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let event = response_create_event(payload)?;
    let session = pool.session(config, endpoint_path, session_id).await?;
    let result = {
        let mut session = session.lock().await;
        session.generate(event).await
    };

    match result {
        Ok(response) => Ok(normalizers::normalize_openai_responses_response(response)),
        Err(error) => {
            pool.close(session_id).await;
            Err(error)
        }
    }
}

impl ResponsesWsSession {
    async fn close(&mut self) -> Result<(), DomainError> {
        self.socket.close(None).await.map_err(|error| {
            DomainError::InternalError(format!("OpenAI Responses WebSocket close failed: {error}"))
        })
    }

    async fn generate(&mut self, event: Value) -> Result<Value, DomainError> {
        self.socket
            .send(Message::Text(event.to_string().into()))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "OpenAI Responses WebSocket send failed: {error}"
                ))
            })?;

        loop {
            let Some(message) = self.socket.next().await else {
                return Err(DomainError::InternalError(
                    "OpenAI Responses WebSocket closed before response.completed".to_string(),
                ));
            };
            let message = message.map_err(|error| {
                DomainError::InternalError(format!(
                    "OpenAI Responses WebSocket read failed: {error}"
                ))
            })?;

            match message {
                Message::Text(text) => {
                    if let Some(response) = response_from_ws_payload(text.as_str().as_bytes())? {
                        return Ok(response);
                    }
                }
                Message::Binary(bytes) => {
                    if let Some(response) = response_from_ws_payload(bytes.as_ref())? {
                        return Ok(response);
                    }
                }
                Message::Ping(bytes) => {
                    self.socket
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|error| {
                            DomainError::InternalError(format!(
                                "OpenAI Responses WebSocket pong failed: {error}"
                            ))
                        })?;
                }
                Message::Close(frame) => {
                    return Err(DomainError::InternalError(format!(
                        "OpenAI Responses WebSocket closed before response.completed: {frame:?}"
                    )));
                }
                Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }
}

async fn generate_ws(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
    let mut socket = connect_responses_ws(config, endpoint_path).await?;
    let event = response_create_event(payload)?;
    socket
        .send(Message::Text(event.to_string().into()))
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("OpenAI Responses WebSocket send failed: {error}"))
        })?;

    loop {
        let Some(message) = socket.next().await else {
            return Err(DomainError::InternalError(
                "OpenAI Responses WebSocket closed before response.completed".to_string(),
            ));
        };
        let message = message.map_err(|error| {
            DomainError::InternalError(format!("OpenAI Responses WebSocket read failed: {error}"))
        })?;

        match message {
            Message::Text(text) => {
                if let Some(response) = response_from_ws_payload(text.as_str().as_bytes())? {
                    return Ok(normalizers::normalize_openai_responses_response(response));
                }
            }
            Message::Binary(bytes) => {
                if let Some(response) = response_from_ws_payload(bytes.as_ref())? {
                    return Ok(normalizers::normalize_openai_responses_response(response));
                }
            }
            Message::Ping(bytes) => {
                socket.send(Message::Pong(bytes)).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "OpenAI Responses WebSocket pong failed: {error}"
                    ))
                })?;
            }
            Message::Close(frame) => {
                return Err(DomainError::InternalError(format!(
                    "OpenAI Responses WebSocket closed before response.completed: {frame:?}"
                )));
            }
            Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

async fn generate_stream_ws(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    sender: ChatCompletionStreamSender,
    mut cancel: ChatCompletionCancelReceiver,
) -> Result<(), ResponsesWsStreamError> {
    let mut socket = connect_responses_ws(config, endpoint_path)
        .await
        .map_err(|error| ResponsesWsStreamError {
            error,
            emitted: false,
        })?;
    let event = response_create_event(payload).map_err(|error| ResponsesWsStreamError {
        error,
        emitted: false,
    })?;
    socket
        .send(Message::Text(event.to_string().into()))
        .await
        .map_err(|error| ResponsesWsStreamError {
            error: DomainError::InternalError(format!(
                "OpenAI Responses WebSocket send failed: {error}"
            )),
            emitted: false,
        })?;

    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut state = ResponsesStreamState::new(model);

    loop {
        if *cancel.borrow() {
            return Ok(());
        }

        let message = tokio::select! {
            _ = cancel.changed() => {
                if *cancel.borrow() {
                    return Ok(());
                }
                continue;
            }
            message = socket.next() => message,
        };

        let Some(message) = message else {
            return Err(ResponsesWsStreamError {
                error: DomainError::InternalError(
                    "OpenAI Responses WebSocket closed before response.completed".to_string(),
                ),
                emitted: state.has_emitted(),
            });
        };
        let message = message.map_err(|error| ResponsesWsStreamError {
            error: DomainError::InternalError(format!(
                "OpenAI Responses WebSocket stream read failed: {error}"
            )),
            emitted: state.has_emitted(),
        })?;

        match message {
            Message::Text(text) => {
                forward_ws_stream_event(&mut state, &sender, text.as_str().as_bytes())?;
            }
            Message::Binary(bytes) => {
                forward_ws_stream_event(&mut state, &sender, bytes.as_ref())?;
            }
            Message::Ping(bytes) => {
                socket.send(Message::Pong(bytes)).await.map_err(|error| {
                    ResponsesWsStreamError {
                        error: DomainError::InternalError(format!(
                            "OpenAI Responses WebSocket pong failed: {error}"
                        )),
                        emitted: state.has_emitted(),
                    }
                })?;
            }
            Message::Close(frame) => {
                return Err(ResponsesWsStreamError {
                    error: DomainError::InternalError(format!(
                        "OpenAI Responses WebSocket closed before response.completed: {frame:?}"
                    )),
                    emitted: state.has_emitted(),
                });
            }
            Message::Pong(_) | Message::Frame(_) => {}
        }

        if state.done_sent {
            return Ok(());
        }
    }
}

fn forward_ws_stream_event(
    state: &mut ResponsesStreamState,
    sender: &ChatCompletionStreamSender,
    payload: &[u8],
) -> Result<(), ResponsesWsStreamError> {
    let event = parse_ws_event(payload).map_err(|error| ResponsesWsStreamError {
        error,
        emitted: state.has_emitted(),
    })?;
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(event_type, "response.failed" | "error") {
        return Err(ResponsesWsStreamError {
            error: response_ws_event_error(&event),
            emitted: state.has_emitted(),
        });
    }

    let payload = serde_json::to_vec(&event).map_err(|error| ResponsesWsStreamError {
        error: DomainError::InternalError(format!(
            "OpenAI Responses WebSocket event serialization failed: {error}"
        )),
        emitted: state.has_emitted(),
    })?;
    state.handle_event(sender, &payload);
    Ok(())
}

async fn connect_responses_ws(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    DomainError,
> {
    let request = build_ws_request(config, endpoint_path)?;
    let (socket, _) = connect_async(request).await.map_err(|error| {
        DomainError::InternalError(format!(
            "OpenAI Responses WebSocket connect failed: {error}"
        ))
    })?;
    Ok(socket)
}

fn build_ws_request(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
) -> Result<WsRequest, DomainError> {
    let ws_url = responses_ws_url(&config.base_url, endpoint_path)?;
    let mut request = ws_url.into_client_request().map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid OpenAI Responses WebSocket request: {error}"
        ))
    })?;

    if let Some(header) = websocket_authorization_header(config) {
        let value = HeaderValue::from_str(&header).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid OpenAI Responses WebSocket Authorization header: {error}"
            ))
        })?;
        request.headers_mut().insert(WS_AUTHORIZATION, value);
    }

    for (key, value) in &config.extra_headers {
        if key.trim().is_empty() || value.trim().is_empty() {
            continue;
        }
        let name = HeaderName::from_bytes(key.as_bytes()).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid OpenAI Responses WebSocket header name {key}: {error}"
            ))
        })?;
        let value = HeaderValue::from_str(value).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid OpenAI Responses WebSocket header value for {key}: {error}"
            ))
        })?;
        request.headers_mut().insert(name, value);
    }

    Ok(request)
}

fn ws_connection_key(
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
) -> Result<String, DomainError> {
    let mut headers = config
        .extra_headers
        .iter()
        .map(|(key, value)| format!("{}={}", key.trim().to_ascii_lowercase(), value.trim()))
        .collect::<Vec<_>>();
    headers.sort_unstable();

    Ok(format!(
        "{}\n{}\n{}",
        responses_ws_url(&config.base_url, endpoint_path)?,
        websocket_authorization_header(config).unwrap_or_default(),
        headers.join("\n")
    ))
}

fn websocket_authorization_header(config: &ChatCompletionApiConfig) -> Option<String> {
    config
        .authorization_header
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let api_key = config.api_key.trim();
            (!api_key.is_empty()).then(|| format!("Bearer {api_key}"))
        })
}

fn responses_ws_url(base_url: &str, endpoint_path: &str) -> Result<String, DomainError> {
    let http_url = HttpChatCompletionRepository::build_url(base_url, endpoint_path);
    let mut url = url::Url::parse(&http_url).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid OpenAI Responses WebSocket URL {http_url}: {error}"
        ))
    })?;
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        "ws" | "wss" => return Ok(url.to_string()),
        other => {
            return Err(DomainError::InvalidData(format!(
                "OpenAI Responses WebSocket URL must use http, https, ws, or wss scheme: {other}"
            )));
        }
    };
    url.set_scheme(scheme).map_err(|_| {
        DomainError::InvalidData(format!("Invalid OpenAI Responses WebSocket URL {http_url}"))
    })?;
    Ok(url.to_string())
}

fn response_create_event(payload: &Value) -> Result<Value, DomainError> {
    let mut event = websocket_response_payload(payload)?;
    event.insert(
        "type".to_string(),
        Value::String("response.create".to_string()),
    );
    Ok(Value::Object(event))
}

fn websocket_response_payload(
    payload: &Value,
) -> Result<serde_json::Map<String, Value>, DomainError> {
    let object = payload.as_object().ok_or_else(|| {
        DomainError::InvalidData("OpenAI Responses payload must be an object".to_string())
    })?;
    let mut response = object.clone();
    response.remove("stream");
    response.remove("background");
    response.remove(CHAT_COMPLETION_PROVIDER_STATE_FIELD);
    Ok(response)
}

fn upstream_payload(payload: &Value) -> Result<Value, DomainError> {
    let mut object = payload.as_object().cloned().ok_or_else(|| {
        DomainError::InvalidData("OpenAI Responses payload must be an object".to_string())
    })?;
    object.remove(CHAT_COMPLETION_PROVIDER_STATE_FIELD);
    Ok(Value::Object(object))
}

fn provider_session_id(payload: &Value) -> Result<Option<String>, DomainError> {
    let Some(provider_state) = payload.get(CHAT_COMPLETION_PROVIDER_STATE_FIELD) else {
        return Ok(None);
    };
    let session_id = provider_state
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            DomainError::InvalidData(
                "OpenAI Responses provider state is missing sessionId".to_string(),
            )
        })?;
    Ok(Some(session_id.to_string()))
}

fn response_from_ws_payload(payload: &[u8]) -> Result<Option<Value>, DomainError> {
    let event = parse_ws_event(payload)?;
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match event_type {
        "response.completed" | "response.done" | "response.incomplete" => {
            let response = event.get("response").cloned().ok_or_else(|| {
                DomainError::InternalError(
                    "OpenAI Responses WebSocket completion event is missing response".to_string(),
                )
            })?;
            Ok(Some(response))
        }
        "response.failed" | "error" => Err(response_ws_event_error(&event)),
        _ => Ok(None),
    }
}

fn parse_ws_event(payload: &[u8]) -> Result<Value, DomainError> {
    serde_json::from_slice(payload).map_err(|error| {
        DomainError::InternalError(format!(
            "OpenAI Responses WebSocket event is not valid JSON: {error}"
        ))
    })
}

fn response_ws_event_error(event: &Value) -> DomainError {
    let message = event
        .get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| {
            event
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("message"))
        })
        .and_then(Value::as_str)
        .unwrap_or("OpenAI Responses WebSocket response failed");

    DomainError::InternalError(message.to_string())
}

fn is_cancelled(error: &DomainError) -> bool {
    matches!(error, DomainError::Cancelled(_))
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::domain::repositories::chat_completion_repository::AnthropicBetaHeaderMode;

    #[test]
    fn responses_ws_url_maps_http_schemes() {
        assert_eq!(
            responses_ws_url("https://api.openai.com/v1", "/responses").unwrap(),
            "wss://api.openai.com/v1/responses"
        );
        assert_eq!(
            responses_ws_url("http://localhost:8080/v1", "/responses").unwrap(),
            "ws://localhost:8080/v1/responses"
        );
    }

    #[test]
    fn response_create_event_removes_http_only_fields() {
        let mut payload = json!({
            "model": "gpt-test",
            "input": [],
            "stream": true,
            "background": false,
            "include": ["reasoning.encrypted_content"]
        });
        payload.as_object_mut().unwrap().insert(
            CHAT_COMPLETION_PROVIDER_STATE_FIELD.to_string(),
            json!({ "sessionId": "run_1" }),
        );
        let event = response_create_event(&payload).unwrap();

        assert_eq!(event["type"], json!("response.create"));
        assert!(event.get("stream").is_none());
        assert!(event.get("background").is_none());
        assert!(event.get(CHAT_COMPLETION_PROVIDER_STATE_FIELD).is_none());
        assert_eq!(event["model"], json!("gpt-test"));
        assert_eq!(event["input"], json!([]));
    }

    #[test]
    fn websocket_request_prefers_explicit_authorization_header() {
        let config = ChatCompletionApiConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "secret".to_string(),
            authorization_header: Some("Bearer override".to_string()),
            extra_headers: HashMap::new(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
        };

        let request = build_ws_request(&config, "/responses").unwrap();

        assert_eq!(
            request
                .headers()
                .get(WS_AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer override")
        );
    }

    #[test]
    fn ws_stream_error_events_surface_before_forwarding_chunks() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut state = ResponsesStreamState::new("gpt-test".to_string());

        let error = forward_ws_stream_event(
            &mut state,
            &sender,
            br#"{"type":"error","error":{"message":"unsupported ws"}}"#,
        )
        .unwrap_err();

        assert!(!error.emitted);
        assert_eq!(error.error.to_string(), "Internal error: unsupported ws");
        assert!(receiver.try_recv().is_err());
    }
}
