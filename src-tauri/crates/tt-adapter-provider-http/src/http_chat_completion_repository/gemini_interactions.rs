use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
};

use super::HttpChatCompletionRepository;
use super::normalizers;
use super::response_body::read_upstream_json_body;

const GEMINI_API_VERSION: &str = "v1beta";

struct ActiveStep {
    index: usize,
    step: Map<String, Value>,
    argument_fragments: String,
}

struct InteractionsStreamState {
    created: u64,
    model: String,
    sent_role: bool,
    saw_text: bool,
    saw_tool_call: bool,
    next_tool_ordinal: usize,
    done_sent: bool,
    completed_steps: Vec<Value>,
    active_step: Option<ActiveStep>,
}

impl InteractionsStreamState {
    fn new(model: String) -> Self {
        Self {
            created: current_unix_timestamp(),
            model,
            sent_role: false,
            saw_text: false,
            saw_tool_call: false,
            next_tool_ordinal: 0,
            done_sent: false,
            completed_steps: Vec::new(),
            active_step: None,
        }
    }

    fn handle_event(
        &mut self,
        sender: &ChatCompletionStreamSender,
        raw_payload: &[u8],
    ) -> Result<(), DomainError> {
        if self.done_sent {
            return Ok(());
        }

        if raw_payload == b"[DONE]" {
            return Err(invalid_stream(
                "received [DONE] before interaction.completed",
            ));
        }

        let event = serde_json::from_slice::<Value>(raw_payload).map_err(|error| {
            DomainError::transient(format!(
                "model.upstream_invalid_response: Gemini Interactions stream event is not valid JSON: {error}"
            ))
        })?;
        let event_object = event
            .as_object()
            .ok_or_else(|| invalid_stream("event must be an object"))?;

        let event_type = required_string(event_object, "event_type", "event")?;

        match event_type {
            "interaction.created"
            | "interaction.status_update"
            | "interaction.in_progress"
            | "interaction.requires_action" => Ok(()),
            "step.start" => self.apply_step_start(event_object, sender),
            "step.delta" => self.apply_step_delta(event_object, sender),
            "step.stop" => self.apply_step_stop(event_object, sender),
            "interaction.completed" => self.apply_interaction_completed(event_object, sender),
            "error" => Err(stream_error(event_object)),
            other => Err(invalid_stream(format!("unsupported event_type {other:?}"))),
        }
    }

    fn apply_step_start(
        &mut self,
        event: &Map<String, Value>,
        sender: &ChatCompletionStreamSender,
    ) -> Result<(), DomainError> {
        if self.active_step.is_some() {
            return Err(invalid_stream(
                "step.start arrived before the active step stopped",
            ));
        }

        let index = required_index(event, "step.start event")?;
        let step = required_object(event, "step", "step.start event")?.clone();
        let step_type = required_string(&step, "type", "step.start step")?;
        if step_type.trim().is_empty() {
            return Err(invalid_stream("step.start step type must not be empty"));
        }

        let projection = match step_type {
            "model_output" => Some((step.get("content"), "model_output.content", "content")),
            "thought" => Some((step.get("summary"), "thought.summary", "reasoning_content")),
            _ => None,
        };

        if let Some((Some(value), context, field)) = projection {
            let blocks = value
                .as_array()
                .ok_or_else(|| invalid_stream(format!("{context} must be an array")))?;
            for block in blocks {
                let block = block
                    .as_object()
                    .ok_or_else(|| invalid_stream(format!("{context} block must be an object")))?;
                if required_string(block, "type", context)? != "text" {
                    return Err(invalid_stream(format!(
                        "{context} only supports text blocks"
                    )));
                }
                let text = required_string(block, "text", context)?;
                if text.is_empty() {
                    continue;
                }
                self.saw_text |= field == "content";
                self.send_delta(sender, projection_delta(field, text));
            }
        }

        self.active_step = Some(ActiveStep {
            index,
            step,
            argument_fragments: String::new(),
        });

        Ok(())
    }

    fn apply_step_delta(
        &mut self,
        event: &Map<String, Value>,
        sender: &ChatCompletionStreamSender,
    ) -> Result<(), DomainError> {
        let index = required_index(event, "step.delta event")?;
        let delta = required_object(event, "delta", "step.delta event")?;
        let delta_type = required_string(delta, "type", "step.delta delta")?;

        let projection = {
            let active = self
                .active_step
                .as_mut()
                .ok_or_else(|| invalid_stream("step.delta arrived without an active step"))?;
            if index != active.index {
                return Err(invalid_stream(format!(
                    "step.delta index {index} does not match active index {}",
                    active.index
                )));
            }

            let step_type = required_string(&active.step, "type", "active step")?;
            match (step_type, delta_type) {
                ("model_output", "text") => {
                    let text = required_string(delta, "text", "text delta")?.to_string();
                    append_text_block(&mut active.step, "content", &text)?;
                    Some(("content", text))
                }
                ("model_output", "text_annotation_delta") => {
                    append_text_annotations(&mut active.step, delta)?;
                    None
                }
                ("thought", "thought_summary") => {
                    let content = required_object(delta, "content", "thought_summary delta")?;
                    if required_string(content, "type", "thought_summary content")? != "text" {
                        return Err(invalid_stream(
                            "thought_summary content must have type text",
                        ));
                    }
                    let text =
                        required_string(content, "text", "thought_summary content")?.to_string();
                    active
                        .step
                        .entry("summary".to_string())
                        .or_insert_with(|| Value::Array(Vec::new()))
                        .as_array_mut()
                        .ok_or_else(|| invalid_stream("thought summary must be an array"))?
                        .push(Value::Object(content.clone()));
                    Some(("reasoning_content", text))
                }
                ("thought", "thought_signature") => {
                    let signature = required_string(delta, "signature", "thought_signature delta")?;
                    active.step.insert(
                        "signature".to_string(),
                        Value::String(signature.to_string()),
                    );
                    None
                }
                ("function_call", "arguments_delta") => {
                    let arguments = required_string(delta, "arguments", "arguments_delta")?;
                    active.argument_fragments.push_str(arguments);
                    None
                }
                ("model_output" | "thought" | "function_call", _) => {
                    return Err(invalid_stream(format!(
                        "delta type {delta_type:?} is invalid for step type {step_type:?}"
                    )));
                }
                _ if delta_type == step_type => {
                    merge_opaque_delta(&mut active.step, delta);
                    None
                }
                _ => {
                    return Err(invalid_stream(format!(
                        "delta type {delta_type:?} does not match opaque step type {step_type:?}"
                    )));
                }
            }
        };

        if let Some((field, text)) = projection
            && !text.is_empty()
        {
            self.saw_text |= field == "content";
            self.send_delta(sender, projection_delta(field, &text));
        }

        Ok(())
    }

    fn apply_step_stop(
        &mut self,
        event: &Map<String, Value>,
        sender: &ChatCompletionStreamSender,
    ) -> Result<(), DomainError> {
        let index = required_index(event, "step.stop event")?;
        let mut active = self
            .active_step
            .take()
            .ok_or_else(|| invalid_stream("step.stop arrived without an active step"))?;
        if index != active.index {
            return Err(invalid_stream(format!(
                "step.stop index {index} does not match active index {}",
                active.index
            )));
        }

        let step_type = required_string(&active.step, "type", "active step")?.to_string();
        let tool_call = if step_type == "function_call" {
            let tool_call = finish_function_call(&mut active, self.next_tool_ordinal)?;
            self.next_tool_ordinal += 1;
            self.saw_tool_call = true;
            Some(tool_call)
        } else {
            None
        };

        self.completed_steps.push(Value::Object(active.step));

        if let Some(tool_call) = tool_call {
            self.send_delta(sender, json!({ "tool_calls": [tool_call] }));
        }

        Ok(())
    }

    fn apply_interaction_completed(
        &mut self,
        event: &Map<String, Value>,
        sender: &ChatCompletionStreamSender,
    ) -> Result<(), DomainError> {
        if self.active_step.is_some() {
            return Err(invalid_stream(
                "interaction.completed arrived before the active step stopped",
            ));
        }

        let interaction = required_object(event, "interaction", "interaction.completed event")?;
        let status = required_string(interaction, "status", "completed interaction")?;
        let finish_reason = match status {
            "completed" if self.saw_tool_call => "tool_calls",
            "completed" => "stop",
            "requires_action" if self.saw_tool_call => "tool_calls",
            "requires_action" => {
                return Err(invalid_stream(
                    "interaction requires action without a function_call step",
                ));
            }
            "incomplete" if self.saw_text && !self.saw_tool_call => "length",
            "incomplete" => {
                return Err(invalid_stream(
                    "incomplete interaction has no consumable partial text",
                ));
            }
            other => {
                return Err(invalid_stream(format!(
                    "interaction did not complete successfully (status: {other})"
                )));
            }
        };

        if !self.saw_text && !self.saw_tool_call {
            return Err(invalid_stream(
                "completed interaction has no consumable text or function_call",
            ));
        }

        let native = json!({
            "gemini_interactions": {
                "steps": std::mem::take(&mut self.completed_steps),
            }
        });
        let usage = normalizers::map_gemini_interactions_usage(interaction.get("usage"));

        self.send_terminal(sender, json!({ "native": native }), finish_reason, usage);
        let _ = sender.send("[DONE]".to_string());
        self.done_sent = true;
        Ok(())
    }

    fn ensure_completed(&self, cancelled: bool) -> Result<(), DomainError> {
        if self.done_sent || cancelled {
            return Ok(());
        }

        Err(DomainError::transient(
            "Gemini Interactions stream closed before interaction.completed".to_string(),
        ))
    }

    fn send_delta(&mut self, sender: &ChatCompletionStreamSender, delta: Value) {
        self.ensure_role(sender);
        self.send_chunk(sender, self.build_chunk(delta, None));
    }

    fn send_terminal(
        &mut self,
        sender: &ChatCompletionStreamSender,
        delta: Value,
        finish_reason: &str,
        usage: Option<Value>,
    ) {
        self.ensure_role(sender);
        let mut chunk = self.build_chunk(delta, Some(finish_reason));
        if let Some(usage) = usage
            && let Some(object) = chunk.as_object_mut()
        {
            object.insert("usage".to_string(), usage);
        }
        self.send_chunk(sender, chunk);
    }

    fn ensure_role(&mut self, sender: &ChatCompletionStreamSender) {
        if self.sent_role {
            return;
        }
        self.sent_role = true;
        self.send_chunk(
            sender,
            self.build_chunk(json!({ "role": "assistant" }), None),
        );
    }

    fn send_chunk(&self, sender: &ChatCompletionStreamSender, chunk: Value) {
        let _ = sender.send(chunk.to_string());
    }

    fn build_chunk(&self, delta: Value, finish_reason: Option<&str>) -> Value {
        json!({
            "id": "gemini-interactions-stream",
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

    normalizers::normalize_gemini_interactions_response(body)
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

    let request = apply_gemini_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
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

    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut state = InteractionsStreamState::new(model);
    let cancelled = cancel.clone();

    let (dummy_sender, dummy_receiver) = mpsc::unbounded_channel::<String>();
    drop(dummy_receiver);

    HttpChatCompletionRepository::stream_sse_response_internal(
        provider_name,
        response,
        dummy_sender,
        cancel,
        |payload| state.handle_event(&sender, payload),
    )
    .await?;

    let was_cancelled = *cancelled.borrow();
    state.ensure_completed(was_cancelled)
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

    HttpChatCompletionRepository::apply_header_if_present(
        request,
        "x-goog-api-key",
        &config.api_key,
    )
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

fn required_object<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    context: &str,
) -> Result<&'a Map<String, Value>, DomainError> {
    object
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_stream(format!("{context} is missing object field {field:?}")))
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    context: &str,
) -> Result<&'a str, DomainError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_stream(format!("{context} is missing string field {field:?}")))
}

fn required_index(object: &Map<String, Value>, context: &str) -> Result<usize, DomainError> {
    let index = object
        .get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_stream(format!("{context} is missing integer index")))?;
    usize::try_from(index)
        .map_err(|_| invalid_stream(format!("{context} index does not fit usize")))
}

fn append_text_block(
    step: &mut Map<String, Value>,
    field: &str,
    text: &str,
) -> Result<(), DomainError> {
    let blocks = step
        .entry(field.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| invalid_stream(format!("step field {field:?} must be an array")))?;

    if let Some(last) = blocks.last_mut().and_then(Value::as_object_mut)
        && last.get("type").and_then(Value::as_str) == Some("text")
    {
        let Some(Value::String(existing)) = last.get_mut("text") else {
            return Err(invalid_stream(format!(
                "{field} text block is missing text"
            )));
        };
        existing.push_str(text);
        return Ok(());
    }

    blocks.push(json!({ "type": "text", "text": text }));
    Ok(())
}

fn append_text_annotations(
    step: &mut Map<String, Value>,
    delta: &Map<String, Value>,
) -> Result<(), DomainError> {
    let annotations = delta
        .get("annotations")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_stream("text_annotation_delta annotations must be an array"))?;
    let content = step
        .get_mut("content")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_stream("text_annotation_delta arrived before text content"))?;
    let text = content
        .last_mut()
        .and_then(Value::as_object_mut)
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .ok_or_else(|| invalid_stream("text_annotation_delta arrived before text content"))?;
    text.entry("annotations".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| invalid_stream("text content annotations must be an array"))?
        .extend(annotations.iter().cloned());
    Ok(())
}

fn merge_opaque_delta(step: &mut Map<String, Value>, delta: &Map<String, Value>) {
    step.extend(
        delta
            .iter()
            .filter(|(key, _)| key.as_str() != "type")
            .map(|(key, value)| (key.clone(), value.clone())),
    );
}

fn finish_function_call(
    active: &mut ActiveStep,
    tool_ordinal: usize,
) -> Result<Value, DomainError> {
    let id = required_string(&active.step, "id", "function_call step")?;
    if id.trim().is_empty() {
        return Err(invalid_stream("function_call id must not be empty"));
    }
    let id = id.to_string();

    let name = required_string(&active.step, "name", "function_call step")?;
    if name.trim().is_empty() {
        return Err(invalid_stream("function_call name must not be empty"));
    }
    let name = name.to_string();

    let arguments_text = if active.argument_fragments.is_empty() {
        let arguments = active
            .step
            .get("arguments")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid_stream("function_call arguments must be an object"))?;
        Value::Object(arguments.clone()).to_string()
    } else {
        let arguments =
            serde_json::from_str::<Value>(&active.argument_fragments).map_err(|error| {
                invalid_stream(format!(
                    "function_call arguments_delta is not valid JSON: {error}"
                ))
            })?;
        if !arguments.is_object() {
            return Err(invalid_stream(
                "function_call arguments_delta must decode to an object",
            ));
        }
        active.step.insert("arguments".to_string(), arguments);
        active.argument_fragments.clone()
    };

    let signature = match active.step.get("signature") {
        None => None,
        Some(Value::String(signature)) => Some(signature.clone()),
        Some(_) => return Err(invalid_stream("function_call signature must be a string")),
    };

    let mut tool_call = json!({
        "index": tool_ordinal,
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments_text,
        }
    });
    if let Some(signature) = signature {
        tool_call["signature"] = Value::String(signature);
    }
    Ok(tool_call)
}

fn projection_delta(field: &str, text: &str) -> Value {
    let mut delta = Map::new();
    delta.insert(field.to_string(), Value::String(text.to_string()));
    Value::Object(delta)
}

fn stream_error(event: &Map<String, Value>) -> DomainError {
    let message = event
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Gemini Interactions stream failed");
    DomainError::transient(message.to_string())
}

fn invalid_stream(message: impl Into<String>) -> DomainError {
    DomainError::InternalError(format!("Gemini Interactions stream {}", message.into()))
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;

    fn apply(
        state: &mut InteractionsStreamState,
        sender: &ChatCompletionStreamSender,
        event: Value,
    ) -> Result<(), DomainError> {
        state.handle_event(sender, &serde_json::to_vec(&event).unwrap())
    }

    fn apply_all(
        state: &mut InteractionsStreamState,
        sender: &ChatCompletionStreamSender,
        events: Value,
    ) {
        for event in events.as_array().unwrap() {
            apply(state, sender, event.clone()).unwrap();
        }
    }

    fn drain(receiver: &mut mpsc::UnboundedReceiver<String>) -> (Vec<Value>, bool) {
        let mut chunks = Vec::new();
        let mut done = false;
        while let Ok(payload) = receiver.try_recv() {
            if payload == "[DONE]" {
                done = true;
            } else {
                chunks.push(serde_json::from_str(&payload).unwrap());
            }
        }
        (chunks, done)
    }

    #[test]
    fn stream_rebuilds_projected_and_opaque_steps() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut state = InteractionsStreamState::new("gemini-3.6-flash".to_string());

        apply_all(
            &mut state,
            &sender,
            json!([
                {
                    "event_type": "interaction.created",
                    "interaction": { "id": "", "status": "in_progress" }
                },
                { "event_type": "interaction.in_progress", "interaction_id": "" },
                { "event_type": "interaction.status_update", "status": "in_progress" },
                {
                    "event_type": "step.start",
                    "index": 0,
                    "step": {
                        "type": "thought",
                        "summary": [{ "type": "text", "text": "think " }]
                    }
                },
                {
                    "event_type": "step.delta",
                    "index": 0,
                    "delta": {
                        "type": "thought_summary",
                        "content": { "type": "text", "text": "more" }
                    }
                },
                {
                    "event_type": "step.delta",
                    "index": 0,
                    "delta": { "type": "thought_signature", "signature": "" }
                },
                { "event_type": "step.stop", "index": 0 },
                {
                    "event_type": "step.start",
                    "index": 1,
                    "step": { "type": "google_search_call", "id": "search_1", "signature": "" }
                },
                {
                    "event_type": "step.delta",
                    "index": 1,
                    "delta": {
                        "type": "google_search_call",
                        "arguments": { "queries": ["Rust"] },
                        "signature": "search_signature"
                    }
                },
                { "event_type": "step.stop", "index": 1 },
                {
                    "event_type": "step.start",
                    "index": 2,
                    "step": {
                        "type": "model_output",
                        "content": [{ "type": "text", "text": "Hel" }]
                    }
                },
                {
                    "event_type": "step.delta",
                    "index": 2,
                    "delta": { "type": "text", "text": "lo" }
                },
                {
                    "event_type": "step.delta",
                    "index": 2,
                    "delta": {
                        "type": "text_annotation_delta",
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://www.rust-lang.org/",
                            "start_index": 0,
                            "end_index": 5
                        }]
                    }
                },
                { "event_type": "step.stop", "index": 2 },
                {
                "event_type": "interaction.completed",
                "interaction": {
                    "id": "",
                    "status": "incomplete",
                    "usage": {
                        "total_input_tokens": 3,
                        "total_output_tokens": 2,
                        "total_thought_tokens": 4,
                        "total_tokens": 9
                    }
                }
                }
            ]),
        );

        let (chunks, done) = drain(&mut receiver);
        let reasoning = chunks
            .iter()
            .filter_map(|chunk| {
                chunk
                    .pointer("/choices/0/delta/reasoning_content")
                    .and_then(Value::as_str)
            })
            .collect::<String>();
        let text = chunks
            .iter()
            .filter_map(|chunk| {
                chunk
                    .pointer("/choices/0/delta/content")
                    .and_then(Value::as_str)
            })
            .collect::<String>();
        let terminal = chunks
            .iter()
            .find(|chunk| chunk.pointer("/choices/0/delta/native").is_some())
            .unwrap();

        assert_eq!(reasoning, "think more");
        assert_eq!(text, "Hello");
        assert_eq!(
            terminal.pointer("/choices/0/delta/native/gemini_interactions/steps/0/summary/0/text"),
            Some(&json!("think "))
        );
        assert_eq!(
            terminal.pointer("/choices/0/delta/native/gemini_interactions/steps/0/summary/1/text"),
            Some(&json!("more"))
        );
        assert_eq!(
            terminal.pointer("/choices/0/delta/native/gemini_interactions/steps/0/signature"),
            Some(&json!(""))
        );
        assert_eq!(
            terminal
                .pointer("/choices/0/delta/native/gemini_interactions/steps/1/arguments/queries/0"),
            Some(&json!("Rust"))
        );
        assert_eq!(
            terminal.pointer("/choices/0/delta/native/gemini_interactions/steps/2/content/0/text"),
            Some(&json!("Hello"))
        );
        assert_eq!(
            terminal.pointer(
                "/choices/0/delta/native/gemini_interactions/steps/2/content/0/annotations/0/type"
            ),
            Some(&json!("url_citation"))
        );
        assert_eq!(terminal["usage"]["prompt_tokens"], json!(3));
        assert_eq!(terminal["usage"]["completion_tokens"], json!(6));
        assert_eq!(terminal["usage"]["total_tokens"], json!(9));
        assert_eq!(terminal["choices"][0]["finish_reason"], json!("length"));
        assert!(done);
        state.ensure_completed(false).unwrap();
    }

    #[test]
    fn stream_emits_completed_function_calls_with_contiguous_ordinals() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut state = InteractionsStreamState::new("gemini-3.6-flash".to_string());

        apply_all(
            &mut state,
            &sender,
            json!([
                {
                    "event_type": "step.start",
                    "index": 0,
                    "step": {
                        "type": "function_call",
                        "id": "call_weather",
                        "name": "get_weather",
                        "arguments": {},
                        "signature": "opaque"
                    }
                },
                {
                    "event_type": "step.delta",
                    "index": 0,
                    "delta": { "type": "arguments_delta", "arguments": "{\"city\":\"" }
                },
                {
                    "event_type": "step.delta",
                    "index": 0,
                    "delta": { "type": "arguments_delta", "arguments": "Paris\"}" }
                }
            ]),
        );
        assert!(receiver.try_recv().is_err());
        apply_all(
            &mut state,
            &sender,
            json!([
                { "event_type": "step.stop", "index": 0 },
                {
                    "event_type": "step.start",
                    "index": 1,
                    "step": {
                        "type": "function_call",
                        "id": "call_time",
                        "name": "get_time",
                        "arguments": {}
                    }
                },
                { "event_type": "step.stop", "index": 1 },
                {
                    "event_type": "interaction.completed",
                    "interaction": { "id": "", "status": "requires_action" }
                }
            ]),
        );

        let (chunks, done) = drain(&mut receiver);
        let tool_calls = chunks
            .iter()
            .filter_map(|chunk| chunk.pointer("/choices/0/delta/tool_calls/0"))
            .collect::<Vec<_>>();
        let terminal = chunks
            .iter()
            .find(|chunk| chunk.pointer("/choices/0/delta/native").is_some())
            .unwrap();

        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0]["index"], json!(0));
        assert_eq!(tool_calls[0]["id"], json!("call_weather"));
        assert_eq!(
            tool_calls[0]["function"]["arguments"],
            json!("{\"city\":\"Paris\"}")
        );
        assert_eq!(tool_calls[0]["signature"], json!("opaque"));
        assert_eq!(tool_calls[1]["index"], json!(1));
        assert_eq!(tool_calls[1]["function"]["arguments"], json!("{}"));
        assert_eq!(
            terminal.pointer("/choices/0/delta/native/gemini_interactions/steps/0/arguments/city"),
            Some(&json!("Paris"))
        );
        assert_eq!(terminal["choices"][0]["finish_reason"], json!("tool_calls"));
        assert!(done);
    }

    #[test]
    fn stream_rejects_invalid_order_payloads_and_premature_eof() {
        let (sender, _receiver) = mpsc::unbounded_channel();

        let mut state = InteractionsStreamState::new("gemini-3.6-flash".to_string());
        let schema_error = apply(
            &mut state,
            &sender,
            json!({
                "event_type": "step.delta",
                "index": 0,
                "delta": { "type": "text", "text": "orphan" }
            }),
        )
        .expect_err("orphan delta must fail");
        assert!(
            matches!(schema_error, DomainError::InternalError(message) if !message.contains("model.upstream_invalid_response"))
        );

        let json_error = state
            .handle_event(&sender, b"{")
            .expect_err("invalid JSON must fail");
        assert!(
            matches!(json_error, DomainError::Transient(message) if message.contains("model.upstream_invalid_response"))
        );

        let mut state = InteractionsStreamState::new("gemini-3.6-flash".to_string());
        apply(
            &mut state,
            &sender,
            json!({
                "event_type": "step.start",
                "index": 0,
                "step": { "type": "thought" }
            }),
        )
        .unwrap();
        assert!(
            apply(
                &mut state,
                &sender,
                json!({
                    "event_type": "step.delta",
                    "index": 0,
                    "delta": { "type": "thought_signature" }
                }),
            )
            .is_err()
        );

        let state = InteractionsStreamState::new("gemini-3.6-flash".to_string());
        assert!(state.ensure_completed(false).is_err());
        state.ensure_completed(true).unwrap();
    }
}
