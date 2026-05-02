use std::collections::HashSet;

use serde_json::{Map, Number, Value, json};

use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::CHAT_COMPLETION_PROVIDER_STATE_FIELD;

use super::shared::{apply_custom_body_overrides, message_content_to_text};
use super::tool_calls::message_tool_call_id;

const REASONING_ENCRYPTED_CONTENT: &str = "reasoning.encrypted_content";

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_raw = payload
        .get("custom_include_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let exclude_raw = payload
        .get("custom_exclude_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let request = build_openai_responses_payload(&payload)?;

    let mut upstream_payload = Value::Object(request);
    apply_custom_body_overrides(&mut upstream_payload, &include_raw, &exclude_raw)?;
    copy_internal_provider_state(&payload, &mut upstream_payload)?;
    finalize_openai_responses_payload(&mut upstream_payload)?;

    Ok(("/responses".to_string(), upstream_payload))
}

fn build_openai_responses_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "OpenAI Responses request is missing model".to_string(),
            )
        })?;

    let previous_response_id = non_empty_string(payload.get("previous_response_id"));
    let input = build_input_items(payload.get("messages"), previous_response_id.is_some())?;

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));
    request.insert("input".to_string(), Value::Array(input));
    request.insert(
        "store".to_string(),
        payload
            .get("store")
            .filter(|value| !value.is_null())
            .cloned()
            .unwrap_or(Value::Bool(false)),
    );

    for key in [
        "stream",
        "temperature",
        "top_p",
        "seed",
        "metadata",
        "parallel_tool_calls",
        "include",
    ] {
        if let Some(value) = payload.get(key).filter(|value| !value.is_null()) {
            request.insert(key.to_string(), value.clone());
        }
    }
    if let Some(previous_response_id) = previous_response_id {
        request.insert(
            "previous_response_id".to_string(),
            Value::String(previous_response_id),
        );
    }

    if let Some(max_tokens) = payload
        .get("max_tokens")
        .or_else(|| payload.get("max_completion_tokens"))
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
    {
        request.insert(
            "max_output_tokens".to_string(),
            Value::Number(Number::from(max_tokens)),
        );
    }

    if let Some(reasoning_effort) = payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"))
    {
        let effort = if reasoning_effort.eq_ignore_ascii_case("min") {
            "minimal"
        } else {
            reasoning_effort
        };
        request.insert("reasoning".to_string(), json!({ "effort": effort }));
    }

    if let Some(verbosity) = payload
        .get("verbosity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"))
    {
        request.insert(
            "verbosity".to_string(),
            Value::String(verbosity.to_string()),
        );
    }

    if let Some(tools) = payload.get("tools").and_then(Value::as_array) {
        if !tools.is_empty() {
            request.insert(
                "tools".to_string(),
                Value::Array(map_openai_tools_to_responses(tools)),
            );

            if let Some(tool_choice) = payload.get("tool_choice") {
                request.insert(
                    "tool_choice".to_string(),
                    map_openai_tool_choice_to_responses(tool_choice.clone()),
                );
            }
        }
    }

    ensure_reasoning_encrypted_include(&mut request)?;

    Ok(request)
}

fn build_input_items(
    messages: Option<&Value>,
    allow_orphan_tool_outputs: bool,
) -> Result<Vec<Value>, ApplicationError> {
    let Some(messages) = messages else {
        return Ok(Vec::new());
    };

    if let Some(prompt) = messages.as_str() {
        return Ok(vec![json!({
            "role": "user",
            "content": prompt,
        })]);
    }

    let entries = messages.as_array().ok_or_else(|| {
        ApplicationError::ValidationError(
            "OpenAI Responses messages must be a string or an array".to_string(),
        )
    })?;

    ResponsesTranscriptCompiler {
        allow_orphan_tool_outputs,
        ..Default::default()
    }
    .compile(entries)
}

#[derive(Default)]
struct ResponsesTranscriptCompiler {
    input: Vec<Value>,
    function_call_ids: HashSet<String>,
    allow_orphan_tool_outputs: bool,
}

impl ResponsesTranscriptCompiler {
    fn compile(mut self, entries: &[Value]) -> Result<Vec<Value>, ApplicationError> {
        for entry in entries {
            let message = entry.as_object().ok_or_else(|| {
                ApplicationError::ValidationError(
                    "OpenAI Responses message entry must be an object".to_string(),
                )
            })?;
            self.compile_message(message)?;
        }

        Ok(self.input)
    }

    fn compile_message(&mut self, message: &Map<String, Value>) -> Result<(), ApplicationError> {
        let role = non_empty_string(message.get("role"))
            .map(|role| role.to_ascii_lowercase())
            .ok_or_else(|| {
                ApplicationError::ValidationError(
                    "OpenAI Responses message entry is missing role".to_string(),
                )
            })?;

        match role.as_str() {
            "assistant" => self.compile_assistant_message(message),
            "tool" | "function" => self.compile_tool_message(message),
            "system" => {
                self.input.push(json!({
                    "role": "developer",
                    "content": message_content_to_text(message.get("content")),
                }));
                Ok(())
            }
            "developer" | "user" => {
                self.input.push(json!({
                    "role": role,
                    "content": message_content_to_text(message.get("content")),
                }));
                Ok(())
            }
            other => Err(ApplicationError::ValidationError(format!(
                "OpenAI Responses message role is unsupported: {other}"
            ))),
        }
    }

    fn compile_assistant_message(
        &mut self,
        message: &Map<String, Value>,
    ) -> Result<(), ApplicationError> {
        if let Some(native_output) = message_native_openai_responses_output(message)? {
            for item in native_output {
                self.remember_function_call_item(&item)?;
                self.input.push(item);
            }
            return Ok(());
        }

        let text = message_content_to_text(message.get("content"));
        if !text.trim().is_empty() {
            self.input.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }],
            }));
        }

        for call in assistant_function_calls(message.get("tool_calls"))? {
            self.function_call_ids.insert(call.call_id.clone());
            self.input.push(call.into_responses_item());
        }

        Ok(())
    }

    fn compile_tool_message(
        &mut self,
        message: &Map<String, Value>,
    ) -> Result<(), ApplicationError> {
        let call_id = message_tool_call_id(message).ok_or_else(|| {
            ApplicationError::ValidationError(
                "Tool message is missing tool_call_id required for Responses function_call_output"
                    .to_string(),
            )
        })?;

        if !self.allow_orphan_tool_outputs && !self.function_call_ids.contains(&call_id) {
            return Err(ApplicationError::ValidationError(format!(
                "Responses function_call_output references call_id without preceding function_call: {call_id}"
            )));
        }

        self.input.push(json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": message_content_to_text(message.get("content")),
        }));

        Ok(())
    }

    fn remember_function_call_item(&mut self, item: &Value) -> Result<(), ApplicationError> {
        let object = item.as_object().ok_or_else(|| {
            ApplicationError::ValidationError(
                "OpenAI Responses native output item must be an object".to_string(),
            )
        })?;

        if object.get("type").and_then(Value::as_str) != Some("function_call") {
            return Ok(());
        }

        let call_id = non_empty_string(object.get("call_id")).ok_or_else(|| {
            ApplicationError::ValidationError(
                "OpenAI Responses native function_call item is missing call_id".to_string(),
            )
        })?;
        self.function_call_ids.insert(call_id);

        Ok(())
    }
}

fn copy_internal_provider_state(
    source: &Map<String, Value>,
    upstream_payload: &mut Value,
) -> Result<(), ApplicationError> {
    let Some(provider_state) = source.get(CHAT_COMPLETION_PROVIDER_STATE_FIELD) else {
        return Ok(());
    };
    let object = upstream_payload.as_object_mut().ok_or_else(|| {
        ApplicationError::InternalError("OpenAI Responses payload must be an object".to_string())
    })?;
    object.insert(
        CHAT_COMPLETION_PROVIDER_STATE_FIELD.to_string(),
        provider_state.clone(),
    );
    Ok(())
}

struct ResponsesFunctionCall {
    call_id: String,
    name: String,
    arguments: String,
}

impl ResponsesFunctionCall {
    fn into_responses_item(self) -> Value {
        json!({
            "type": "function_call",
            "call_id": self.call_id,
            "name": self.name,
            "arguments": self.arguments,
        })
    }
}

fn assistant_function_calls(
    value: Option<&Value>,
) -> Result<Vec<ResponsesFunctionCall>, ApplicationError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    let calls = value.as_array().ok_or_else(|| {
        ApplicationError::ValidationError(
            "Assistant tool_calls must be an array for OpenAI Responses".to_string(),
        )
    })?;

    calls
        .iter()
        .map(|call| {
            let object = call.as_object().ok_or_else(|| {
                ApplicationError::ValidationError(
                    "Assistant tool_call entry must be an object".to_string(),
                )
            })?;
            let function = object
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "Assistant tool_call entry is missing function".to_string(),
                    )
                })?;

            let call_id = non_empty_string(object.get("id")).ok_or_else(|| {
                ApplicationError::ValidationError(
                    "Assistant tool_call entry is missing id".to_string(),
                )
            })?;
            let name = non_empty_string(function.get("name")).ok_or_else(|| {
                ApplicationError::ValidationError(
                    "Assistant tool_call function is missing name".to_string(),
                )
            })?;
            let arguments = function_call_arguments(function.get("arguments"))?;

            Ok(ResponsesFunctionCall {
                call_id,
                name,
                arguments,
            })
        })
        .collect()
}

fn function_call_arguments(value: Option<&Value>) -> Result<String, ApplicationError> {
    match value {
        Some(Value::String(arguments)) => Ok(arguments.clone()),
        Some(Value::Null) | None => Ok("{}".to_string()),
        Some(value) => serde_json::to_string(value).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "Assistant tool_call arguments are not serializable: {error}"
            ))
        }),
    }
}

fn message_native_openai_responses_output(
    message: &Map<String, Value>,
) -> Result<Option<Vec<Value>>, ApplicationError> {
    let Some(native) = message.get("native") else {
        return Ok(None);
    };
    let Some(openai_responses) = native.get("openai_responses") else {
        return Ok(None);
    };

    let output = openai_responses
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "OpenAI Responses native metadata is missing output array".to_string(),
            )
        })?;

    Ok(Some(output))
}

fn finalize_openai_responses_payload(payload: &mut Value) -> Result<(), ApplicationError> {
    let object = payload.as_object_mut().ok_or_else(|| {
        ApplicationError::InternalError("OpenAI Responses payload must be an object".to_string())
    })?;

    validate_openai_responses_payload(object)?;
    ensure_reasoning_encrypted_include(object)?;
    validate_openai_responses_payload(object)
}

fn validate_openai_responses_payload(object: &Map<String, Value>) -> Result<(), ApplicationError> {
    non_empty_string(object.get("model")).ok_or_else(|| {
        ApplicationError::ValidationError("OpenAI Responses payload is missing model".to_string())
    })?;

    if object.get("input").and_then(Value::as_array).is_none() {
        return Err(ApplicationError::ValidationError(
            "OpenAI Responses payload is missing input array".to_string(),
        ));
    }

    if let Some(store) = object.get("store") {
        if !store.is_boolean() {
            return Err(ApplicationError::ValidationError(
                "OpenAI Responses store must be a boolean".to_string(),
            ));
        }
    }

    if let Some(include) = object.get("include") {
        validate_include(include)?;
    }

    Ok(())
}

fn ensure_reasoning_encrypted_include(
    object: &mut Map<String, Value>,
) -> Result<(), ApplicationError> {
    let entry = object
        .entry("include".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let items = entry.as_array_mut().ok_or_else(|| {
        ApplicationError::ValidationError("OpenAI Responses include must be an array".to_string())
    })?;
    let encrypted = Value::String(REASONING_ENCRYPTED_CONTENT.to_string());
    if !items.iter().any(|item| item == &encrypted) {
        items.push(encrypted);
    }

    Ok(())
}

fn validate_include(include: &Value) -> Result<(), ApplicationError> {
    let items = include.as_array().ok_or_else(|| {
        ApplicationError::ValidationError("OpenAI Responses include must be an array".to_string())
    })?;

    if items.iter().all(Value::is_string) {
        Ok(())
    } else {
        Err(ApplicationError::ValidationError(
            "OpenAI Responses include entries must be strings".to_string(),
        ))
    }
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn map_openai_tools_to_responses(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| tool.as_object())
        .map(|tool| {
            let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or_default();

            if tool_type != "function" {
                return Value::Object(tool.clone());
            }

            let strict = tool
                .get("strict")
                .and_then(Value::as_bool)
                .or_else(|| {
                    tool.get("function")
                        .and_then(Value::as_object)
                        .and_then(|f| f.get("strict"))
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false);

            if let Some(function) = tool.get("function").and_then(Value::as_object) {
                let mut mapped = Map::new();
                mapped.insert("type".to_string(), Value::String("function".to_string()));
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    mapped.insert("name".to_string(), Value::String(name.to_string()));
                }
                if let Some(description) = function.get("description").and_then(Value::as_str) {
                    mapped.insert(
                        "description".to_string(),
                        Value::String(description.to_string()),
                    );
                }
                if let Some(parameters) = function.get("parameters") {
                    mapped.insert("parameters".to_string(), parameters.clone());
                }
                mapped.insert("strict".to_string(), Value::Bool(strict));
                return Value::Object(mapped);
            }

            let mut mapped = Map::new();
            mapped.insert("type".to_string(), Value::String("function".to_string()));
            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                mapped.insert("name".to_string(), Value::String(name.to_string()));
            }
            if let Some(description) = tool.get("description").and_then(Value::as_str) {
                mapped.insert(
                    "description".to_string(),
                    Value::String(description.to_string()),
                );
            }
            if let Some(parameters) = tool.get("parameters") {
                mapped.insert("parameters".to_string(), parameters.clone());
            }
            mapped.insert("strict".to_string(), Value::Bool(strict));
            Value::Object(mapped)
        })
        .collect()
}

fn map_openai_tool_choice_to_responses(tool_choice: Value) -> Value {
    let Value::Object(object) = tool_choice else {
        return tool_choice;
    };

    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if tool_type == "function" {
        if let Some(function) = object.get("function").and_then(Value::as_object) {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                return json!({
                    "type": "function",
                    "name": name,
                });
            }
        }

        if object.get("name").and_then(Value::as_str).is_some() {
            return Value::Object(object);
        }
    }

    if tool_type == "allowed_tools" {
        let mut mapped = object.clone();
        if let Some(tools) = mapped.get_mut("tools").and_then(Value::as_array_mut) {
            for tool in tools.iter_mut() {
                let Value::Object(tool_object) = tool else {
                    continue;
                };
                if tool_object.get("type").and_then(Value::as_str) != Some("function") {
                    continue;
                }
                if tool_object.get("name").and_then(Value::as_str).is_some() {
                    continue;
                }
                if let Some(function) = tool_object.get("function").and_then(Value::as_object) {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        tool_object.insert("name".to_string(), Value::String(name.to_string()));
                    }
                }
                tool_object.remove("function");
            }
        }
        return Value::Object(mapped);
    }

    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn openai_responses_payload_maps_system_and_tool_turns_to_typed_items() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "hi" },
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Paris\"}"
                        }
                    }]
                },
                { "role": "tool", "tool_call_id": "call_123", "content": "ok" }
            ],
            "include": ["file_search_call.results"],
            "stream": true,
            "custom_url": "https://example.com/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/responses");

        let request = upstream.as_object().expect("request must be object");
        assert_eq!(request.get("model").and_then(|v| v.as_str()), Some("gpt-5"));

        let input = request
            .get("input")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        assert_eq!(input[0]["role"], "developer");
        assert_eq!(input[0]["content"], "sys");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["call_id"], "call_123");
        assert_eq!(input[2]["name"], "get_weather");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_123");
        assert_eq!(input[3]["output"], "ok");

        let include = request
            .get("include")
            .and_then(Value::as_array)
            .expect("include should exist");
        assert!(
            include
                .iter()
                .any(|value| value == "file_search_call.results")
        );
        assert!(
            include
                .iter()
                .any(|value| value == "reasoning.encrypted_content")
        );
    }

    #[test]
    fn openai_responses_payload_lifts_function_tools() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [{ "role": "user", "content": "hi" }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "desc",
                    "parameters": { "type": "object", "properties": {} }
                }
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let request = upstream.as_object().expect("request must be object");
        let tools = request.get("tools").and_then(|v| v.as_array()).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["strict"], false);
    }

    #[test]
    fn openai_responses_payload_replays_native_output_items() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "user", "content": "hi" },
                {
                    "role": "assistant",
                    "content": "",
                    "native": {
                        "openai_responses": {
                            "responseId": "resp_1",
                            "output": [{
                                "id": "fc_1",
                                "type": "function_call",
                                "call_id": "call_1",
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"hi\"}"
                            }]
                        }
                    }
                },
                { "role": "tool", "tool_call_id": "call_1", "content": "ok" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let input = upstream
            .get("input")
            .and_then(Value::as_array)
            .expect("input should exist");

        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_1");
    }

    #[test]
    fn openai_responses_payload_keeps_non_trailing_tool_outputs_typed() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "user", "content": "start" },
                {
                    "role": "assistant",
                    "content": "",
                    "native": {
                        "openai_responses": {
                            "responseId": "resp_1",
                            "output": [{
                                "id": "fc_1",
                                "type": "function_call",
                                "call_id": "call_1",
                                "name": "workspace_write_file",
                                "arguments": "{\"path\":\"output/main.md\",\"content\":\"draft\"}"
                            }]
                        }
                    }
                },
                { "role": "tool", "tool_call_id": "call_1", "content": "wrote draft" },
                {
                    "role": "assistant",
                    "content": "checking",
                    "native": {
                        "openai_responses": {
                            "responseId": "resp_2",
                            "output": [{
                                "id": "fc_2",
                                "type": "function_call",
                                "call_id": "call_2",
                                "name": "workspace_read_file",
                                "arguments": "{\"path\":\"output/main.md\"}"
                            }]
                        }
                    }
                },
                { "role": "tool", "tool_call_id": "call_2", "content": "draft" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let input = upstream
            .get("input")
            .and_then(Value::as_array)
            .expect("input should exist");

        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_1");
        assert_eq!(input[4]["type"], "function_call_output");
        assert_eq!(input[4]["call_id"], "call_2");
    }

    #[test]
    fn openai_responses_payload_readds_reasoning_include_after_overrides() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [{ "role": "user", "content": "hi" }],
            "custom_include_body": "{\"include\":[\"file_search_call.results\"]}"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let include = upstream
            .get("include")
            .and_then(Value::as_array)
            .expect("include should exist");

        assert!(
            include
                .iter()
                .any(|value| value == "file_search_call.results")
        );
        assert!(
            include
                .iter()
                .any(|value| value == "reasoning.encrypted_content")
        );
    }

    #[test]
    fn openai_responses_payload_rejects_orphan_tool_output() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "tool", "tool_call_id": "call_1", "content": "orphan" }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("without preceding function_call")
        );
    }

    #[test]
    fn openai_responses_payload_allows_incremental_tool_output_after_previous_response() {
        let payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "openai_responses",
            "model": "gpt-5",
            "previous_response_id": "resp_123",
            "messages": [
                { "role": "tool", "tool_call_id": "call_123", "content": "ok" }
            ],
            "custom_url": "https://example.com/v1"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        assert_eq!(upstream["previous_response_id"], json!("resp_123"));
        assert_eq!(
            upstream["input"],
            json!([{
                "type": "function_call_output",
                "call_id": "call_123",
                "output": "ok"
            }])
        );
    }
}
