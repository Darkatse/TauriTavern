use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Map, Value};

use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamSender,
    ChatCompletionToolCallDelta,
};

use super::HttpChatCompletionRepository;
use super::response_body::read_upstream_json_body;

pub(super) async fn list_models(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    provider_name: &str,
) -> Result<Value, DomainError> {
    list_models_with_path(repository, config, provider_name, "/models").await
}

pub(super) async fn list_models_with_path(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    provider_name: &str,
    path: &str,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, path)?;

    let client = repository.metadata_client(config)?;
    let request = client.get(url).header(ACCEPT, "application/json");
    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
    let request = HttpChatCompletionRepository::apply_extra_headers(request, &config.extra_headers);
    let request = HttpChatCompletionRepository::apply_additional_headers(request, config);

    let response = request.send().await.map_err(|error| {
        HttpChatCompletionRepository::map_transport_error("Status request failed", error)
    })?;

    if !response.status().is_success() {
        return Err(HttpChatCompletionRepository::map_error_response(
            provider_name,
            response,
            "Failed to list models",
        )
        .await);
    }

    read_upstream_json_body(provider_name, "list_models", response).await
}

pub(super) async fn generate(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
) -> Result<Value, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path)?;

    let client = repository.client(config)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
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

    if super::payload_contains_cache_control(payload) {
        let model = payload.get("model").and_then(Value::as_str);
        let _ = super::log_prompt_cache_performance_if_present(provider_name, model, &body);
    }

    Ok(body)
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
    let mut accumulator = OpenAiChatAccumulator::default();

    HttpChatCompletionRepository::consume_sse_response(provider_name, response, |event| {
        accumulator.apply_event(event, on_tool_call_delta)
    })
    .await?;

    accumulator.finish()
}

async fn send_stream_request(
    repository: &HttpChatCompletionRepository,
    config: &ChatCompletionApiConfig,
    endpoint_path: &str,
    payload: &Value,
    provider_name: &str,
) -> Result<reqwest::Response, DomainError> {
    let url = HttpChatCompletionRepository::build_url(&config.base_url, endpoint_path)?;

    let client = repository.stream_client(config)?;
    let request = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(payload);

    let request = HttpChatCompletionRepository::apply_openai_auth(request, config);
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

    Ok(response)
}

#[derive(Default)]
struct OpenAiToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct OpenAiChatAccumulator {
    id: Option<String>,
    created: Option<u64>,
    model: Option<String>,
    service_tier: Option<String>,
    system_fingerprint: Option<String>,
    usage: Option<Value>,
    content: String,
    refusal: String,
    reasoning: String,
    reasoning_content: String,
    reasoning_details: Vec<Value>,
    finish_reason: Option<String>,
    tool_calls: Vec<OpenAiToolCallAccumulator>,
}

impl OpenAiChatAccumulator {
    fn apply_event(
        &mut self,
        raw_event: &[u8],
        on_tool_call_delta: &mut dyn FnMut(ChatCompletionToolCallDelta),
    ) -> Result<(), DomainError> {
        if raw_event == b"[DONE]" {
            return Ok(());
        }

        let mut event = serde_json::from_slice::<Value>(raw_event)
            .map_err(|error| invalid_openai_stream(format!("event is not valid JSON: {error}")))?;
        let event = event
            .as_object_mut()
            .ok_or_else(|| invalid_openai_stream("event must be an object"))?;

        if let Some(error) = event.get("error").filter(|error| !error.is_null()) {
            return Err(invalid_openai_stream(format!(
                "upstream returned an error event: {error}"
            )));
        }

        if self.id.is_none() {
            self.id = take_optional_string(event, "id")?;
        }
        if self.created.is_none() {
            self.created = event.get("created").and_then(Value::as_u64);
        }
        if self.model.is_none() {
            self.model = take_optional_string(event, "model")?;
        }
        if self.service_tier.is_none() {
            self.service_tier = take_optional_string(event, "service_tier")?;
        }
        if self.system_fingerprint.is_none() {
            self.system_fingerprint = take_optional_string(event, "system_fingerprint")?;
        }

        if let Some(usage) = event.remove("usage").filter(|value| !value.is_null()) {
            self.usage = Some(usage);
        }

        let choices = event
            .get_mut("choices")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| invalid_openai_stream("event is missing choices"))?;
        if choices.is_empty() {
            return Ok(());
        }
        if choices.len() != 1 {
            return Err(invalid_openai_stream(
                "Agent stream must contain exactly one choice",
            ));
        }

        let choice = choices[0]
            .as_object_mut()
            .ok_or_else(|| invalid_openai_stream("choice must be an object"))?;
        if choice.get("index").and_then(Value::as_u64) != Some(0) {
            return Err(invalid_openai_stream("Agent stream choice index must be 0"));
        }
        if let Some(value) = take_optional_string(choice, "finish_reason")? {
            self.finish_reason = Some(value);
        }

        let delta = choice
            .get_mut("delta")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| invalid_openai_stream("choice is missing delta"))?;
        if let Some(value) = take_optional_string(delta, "content")? {
            append_string_fragment(&mut self.content, value);
        }
        if let Some(value) = take_optional_string(delta, "refusal")? {
            append_string_fragment(&mut self.refusal, value);
        }
        if let Some(value) = take_optional_string(delta, "reasoning")? {
            append_string_fragment(&mut self.reasoning, value);
        }
        if let Some(value) = take_optional_string(delta, "reasoning_content")? {
            append_string_fragment(&mut self.reasoning_content, value);
        }
        match delta.remove("reasoning_details") {
            None | Some(Value::Null) => {}
            Some(Value::Array(details)) => self.reasoning_details.extend(details),
            Some(_) => {
                return Err(invalid_openai_stream(
                    "delta.reasoning_details must be an array",
                ));
            }
        }

        if let Some(tool_calls) = delta.get_mut("tool_calls") {
            let tool_calls = tool_calls
                .as_array_mut()
                .ok_or_else(|| invalid_openai_stream("delta.tool_calls must be an array"))?;
            for tool_call in tool_calls {
                self.apply_tool_call_delta(tool_call, on_tool_call_delta)?;
            }
        }

        Ok(())
    }

    fn apply_tool_call_delta(
        &mut self,
        raw_delta: &mut Value,
        on_tool_call_delta: &mut dyn FnMut(ChatCompletionToolCallDelta),
    ) -> Result<(), DomainError> {
        let delta = raw_delta
            .as_object_mut()
            .ok_or_else(|| invalid_openai_stream("tool call delta must be an object"))?;
        let tool_call_index = delta
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| invalid_openai_stream("tool call delta is missing index"))?;

        if tool_call_index == self.tool_calls.len() {
            self.tool_calls.push(OpenAiToolCallAccumulator::default());
        } else if tool_call_index > self.tool_calls.len() {
            return Err(invalid_openai_stream(format!(
                "tool call index {tool_call_index} skipped the next index {}",
                self.tool_calls.len()
            )));
        }

        let state = &mut self.tool_calls[tool_call_index];
        if state.id.is_empty()
            && let Some(id) = take_optional_string(delta, "id")?
        {
            state.id = id;
        }

        let function = match delta.get_mut("function") {
            None | Some(Value::Null) => None,
            Some(Value::Object(function)) => Some(function),
            Some(_) => {
                return Err(invalid_openai_stream(
                    "tool call delta function must be an object",
                ));
            }
        };
        let Some(function) = function else {
            return Ok(());
        };

        if state.name.is_empty() {
            if let Some(name) = take_optional_string(function, "name")? {
                state.name = name;
            }
            if !state.name.is_empty() && !state.arguments.is_empty() {
                on_tool_call_delta(ChatCompletionToolCallDelta {
                    tool_call_index,
                    name: state.name.clone(),
                    arguments_fragment: state.arguments.clone(),
                });
            }
        }
        let Some(arguments_fragment) = take_optional_string(function, "arguments")? else {
            return Ok(());
        };
        if arguments_fragment.is_empty() {
            return Ok(());
        }
        state.arguments.push_str(&arguments_fragment);
        if state.name.is_empty() {
            return Ok(());
        }
        on_tool_call_delta(ChatCompletionToolCallDelta {
            tool_call_index,
            name: state.name.clone(),
            arguments_fragment,
        });
        Ok(())
    }

    fn finish(self) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
        let finish_reason = self
            .finish_reason
            .ok_or_else(|| invalid_openai_stream("ended without a finish reason"))?;
        if finish_reason == "tool_calls" && self.tool_calls.is_empty() {
            return Err(invalid_openai_stream(
                "ended with tool_calls finish reason but no tool calls",
            ));
        }

        let mut message = Map::new();
        message.insert("role".to_string(), Value::String("assistant".to_string()));
        message.insert(
            "content".to_string(),
            if self.content.is_empty() {
                Value::Null
            } else {
                Value::String(self.content)
            },
        );
        if !self.refusal.is_empty() {
            message.insert("refusal".to_string(), Value::String(self.refusal));
        }
        if !self.reasoning.is_empty() {
            message.insert("reasoning".to_string(), Value::String(self.reasoning));
        }
        if !self.reasoning_content.is_empty() {
            message.insert(
                "reasoning_content".to_string(),
                Value::String(self.reasoning_content),
            );
        }
        if !self.reasoning_details.is_empty() {
            message.insert(
                "reasoning_details".to_string(),
                Value::Array(self.reasoning_details),
            );
        }
        if !self.tool_calls.is_empty() {
            message.insert(
                "tool_calls".to_string(),
                Value::Array(
                    self.tool_calls
                        .into_iter()
                        .map(|tool_call| {
                            let mut call = Map::new();
                            if !tool_call.id.is_empty() {
                                call.insert("id".to_string(), Value::String(tool_call.id));
                            }
                            call.insert("type".to_string(), Value::String("function".to_string()));
                            let mut function = Map::new();
                            function.insert("name".to_string(), Value::String(tool_call.name));
                            function.insert(
                                "arguments".to_string(),
                                Value::String(tool_call.arguments),
                            );
                            call.insert("function".to_string(), Value::Object(function));
                            Value::Object(call)
                        })
                        .collect(),
                ),
            );
        }

        let mut body = Map::new();
        body.insert(
            "object".to_string(),
            Value::String("chat.completion".to_string()),
        );
        let mut choice = Map::new();
        choice.insert("index".to_string(), Value::from(0));
        choice.insert("message".to_string(), Value::Object(message));
        choice.insert("finish_reason".to_string(), Value::String(finish_reason));
        body.insert(
            "choices".to_string(),
            Value::Array(vec![Value::Object(choice)]),
        );
        if let Some(value) = self.id {
            body.insert("id".to_string(), Value::String(value));
        }
        if let Some(value) = self.created {
            body.insert("created".to_string(), Value::from(value));
        }
        if let Some(value) = self.model {
            body.insert("model".to_string(), Value::String(value));
        }
        if let Some(value) = self.service_tier {
            body.insert("service_tier".to_string(), Value::String(value));
        }
        if let Some(value) = self.system_fingerprint {
            body.insert("system_fingerprint".to_string(), Value::String(value));
        }
        if let Some(value) = self.usage {
            body.insert("usage".to_string(), value);
        }

        Ok(ChatCompletionRepositoryGenerateResponse::from_body(
            Value::Object(body),
        ))
    }
}

fn take_optional_string(
    object: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<String>, DomainError> {
    match object.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(invalid_openai_stream(format!(
            "field `{key}` must be a string or null"
        ))),
    }
}

fn append_string_fragment(target: &mut String, fragment: String) {
    if target.is_empty() {
        *target = fragment;
    } else {
        target.push_str(&fragment);
    }
}

fn invalid_openai_stream(message: impl std::fmt::Display) -> DomainError {
    DomainError::transient(format!(
        "model.upstream_invalid_response: OpenAI Chat stream {message}"
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::OpenAiChatAccumulator;
    use tt_ports::repositories::chat_completion_repository::ChatCompletionToolCallDelta;

    #[test]
    fn openai_chat_stream_projects_tool_fragments_and_builds_agent_final() {
        let events = [
            br#"{"id":"chat_1","object":"chat.completion.chunk","created":42,"model":"gpt-test","choices":[{"index":0,"delta":{"role":"assistant","content":"I will ","reasoning":"Plan ","reasoning_content":"Need ","reasoning_details":[{"type":"reasoning.encrypted","id":"call_0","data":"opaque"}],"tool_calls":[{"index":0,"id":"call_0","type":"function","function":{"arguments":"{\"path\":\"a.md\",\"content\":\"hel"}}]},"finish_reason":null}]}"#.as_slice(),
            br#"{"id":"chat_1","created":42,"model":"gpt-test","error":null,"choices":[{"index":0,"delta":{"content":"write.","reasoning":"then act.","reasoning_content":"files.","tool_calls":[{"index":0,"function":{"name":"workspace_write_file"}},{"index":1,"id":"call_1","type":"function","function":{"name":"workspace_apply_patch","arguments":"{\"path\":\"b.md\",\"old_string\":\"x\",\"new_string\":\"n"}}]},"finish_reason":null}]}"#.as_slice(),
            br#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"lo\"}"}}]},"finish_reason":null}]}"#.as_slice(),
            br#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"ew\"}"}}]},"finish_reason":null}]}"#.as_slice(),
            br#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#.as_slice(),
            br#"{"choices":[]}"#.as_slice(),
            br#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#.as_slice(),
            b"[DONE]".as_slice(),
        ];
        let mut accumulator = OpenAiChatAccumulator::default();
        let mut deltas = Vec::<ChatCompletionToolCallDelta>::new();
        for event in events {
            accumulator
                .apply_event(event, &mut |delta| deltas.push(delta))
                .unwrap();
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
                    tool_call_index: 1,
                    name: "workspace_apply_patch".to_string(),
                    arguments_fragment:
                        "{\"path\":\"b.md\",\"old_string\":\"x\",\"new_string\":\"n".to_string(),
                },
                ChatCompletionToolCallDelta {
                    tool_call_index: 0,
                    name: "workspace_write_file".to_string(),
                    arguments_fragment: "lo\"}".to_string(),
                },
                ChatCompletionToolCallDelta {
                    tool_call_index: 1,
                    name: "workspace_apply_patch".to_string(),
                    arguments_fragment: "ew\"}".to_string(),
                },
            ]
        );

        let body = accumulator.finish().unwrap().body;
        assert_eq!(body["id"], "chat_1");
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["choices"][0]["message"]["content"], "I will write.");
        assert_eq!(body["choices"][0]["message"]["reasoning"], "Plan then act.");
        assert_eq!(
            body["choices"][0]["message"]["reasoning_content"],
            "Need files."
        );
        assert_eq!(
            body["choices"][0]["message"]["reasoning_details"],
            json!([{"type":"reasoning.encrypted","id":"call_0","data":"opaque"}])
        );
        assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            body["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a.md\",\"content\":\"hello\"}"
        );
        assert_eq!(
            body["choices"][0]["message"]["tool_calls"][1]["function"]["arguments"],
            "{\"path\":\"b.md\",\"old_string\":\"x\",\"new_string\":\"new\"}"
        );
        assert_eq!(
            body["usage"],
            json!({
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
            })
        );

        let mut clean_eof = OpenAiChatAccumulator::default();
        clean_eof
            .apply_event(
                br#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
                &mut |_| {},
            )
            .unwrap();
        assert!(clean_eof.finish().is_ok());

        let mut missing_finish = OpenAiChatAccumulator::default();
        missing_finish.apply_event(b"[DONE]", &mut |_| {}).unwrap();
        assert!(missing_finish.finish().is_err());

        let mut missing_tool_calls = OpenAiChatAccumulator::default();
        missing_tool_calls
            .apply_event(
                br#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                &mut |_| {},
            )
            .unwrap();
        assert!(missing_tool_calls.finish().is_err());
    }
}
