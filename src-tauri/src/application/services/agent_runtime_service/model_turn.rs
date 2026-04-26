use serde_json::{Map, Value, json};

use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::BuiltinAgentToolRegistry;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

pub(super) fn extract_agent_tool_calls(
    response: &Value,
    registry: &BuiltinAgentToolRegistry,
) -> Result<Vec<AgentToolCall>, ApplicationError> {
    let Some(calls) = response
        .pointer("/choices/0/message/tool_calls")
        .or_else(|| response.pointer("/message/tool_calls"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    calls
        .iter()
        .enumerate()
        .map(|(index, call)| parse_agent_tool_call(index, call, registry))
        .collect()
}

pub(super) fn assistant_message_for_next_turn(response: &Value) -> Result<Value, ApplicationError> {
    let message = response
        .pointer("/choices/0/message")
        .or_else(|| response.pointer("/message"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_response: response is missing assistant message".to_string(),
            )
        })?;
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .filter(|calls| !calls.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_response: assistant message is missing tool_calls".to_string(),
            )
        })?;

    Ok(json!({
        "role": "assistant",
        "content": message.get("content").cloned().unwrap_or(Value::Null),
        "tool_calls": tool_calls,
    }))
}

pub(super) fn append_tool_turn_to_request(
    request: &mut ChatCompletionGenerateRequestDto,
    assistant_message: Value,
    tool_results: &[AgentToolResult],
) -> Result<(), ApplicationError> {
    let messages = request
        .payload
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.tool_loop_requires_messages: messages must be an array".to_string(),
            )
        })?;

    messages.push(assistant_message);
    for result in tool_results {
        messages.push(json!({
            "role": "tool",
            "tool_call_id": result.call_id.as_str(),
            "name": result.name.as_str(),
            "content": tool_result_message_content(result)?,
        }));
    }

    Ok(())
}

pub(super) fn extract_response_text(response: &Value) -> Result<String, ApplicationError> {
    if let Some(text) = response.as_str() {
        return Ok(text.to_string());
    }

    for pointer in [
        "/choices/0/message/content",
        "/choices/0/text",
        "/text",
        "/message/content",
        "/message/tool_plan",
        "/output",
        "/content",
    ] {
        if let Some(text) = text_from_value(response.pointer(pointer)) {
            return Ok(text);
        }
    }

    Err(ApplicationError::ValidationError(
        "model.empty_response_text: could not extract assistant message text from model response"
            .to_string(),
    ))
}

fn parse_agent_tool_call(
    index: usize,
    call: &Value,
    registry: &BuiltinAgentToolRegistry,
) -> Result<AgentToolCall, ApplicationError> {
    let object = call.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "model.invalid_tool_call: tool call must be an object".to_string(),
        )
    })?;
    let function = object
        .get("function")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_call: tool call is missing function".to_string(),
            )
        })?;
    let raw_name = function
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_call: tool call function name is required".to_string(),
            )
        })?;
    let canonical_name = registry.canonical_name(raw_name).unwrap_or(raw_name);
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("tool_call_{index}"));
    let arguments =
        parse_tool_call_arguments(function.get("arguments").or_else(|| function.get("args")));

    Ok(AgentToolCall {
        id,
        name: canonical_name.to_string(),
        arguments,
        provider_metadata: json!({
            "modelName": raw_name,
            "signature": object.get("signature"),
            "raw": call,
        }),
    })
}

fn parse_tool_call_arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(raw)) => {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
        }
        Some(Value::Null) | None => Value::Object(Map::new()),
        Some(value) => value.clone(),
    }
}

fn tool_result_message_content(result: &AgentToolResult) -> Result<String, ApplicationError> {
    serde_json::to_string(&json!({
        "ok": !result.is_error,
        "content": result.content.as_str(),
        "structured": &result.structured,
        "errorCode": result.error_code.as_deref(),
        "resourceRefs": &result.resource_refs,
    }))
    .map_err(|error| {
        ApplicationError::ValidationError(format!("agent.tool_result_serialize_failed: {error}"))
    })
}

fn text_from_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut output = String::new();
            for part in parts {
                match part {
                    Value::String(text) => output.push_str(text),
                    Value::Object(object) => {
                        if object.get("type").and_then(Value::as_str) == Some("tool_use") {
                            return None;
                        }
                        if let Some(text) = object.get("text").and_then(Value::as_str) {
                            output.push_str(text);
                        } else if let Some(text) = object.get("content").and_then(Value::as_str) {
                            output.push_str(text);
                        }
                    }
                    _ => {}
                }
            }
            Some(output)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::agent_tools::BuiltinAgentToolRegistry;

    #[test]
    fn extracts_openai_message_content() {
        let response = json!({
            "choices": [{
                "message": { "content": "hello" }
            }]
        });

        assert_eq!(extract_response_text(&response).unwrap(), "hello");
    }

    #[test]
    fn extracts_tool_call_response() {
        let response = json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"hello\"}"
                        }
                    }]
                }
            }]
        });

        let registry = BuiltinAgentToolRegistry::phase2a();
        let calls = extract_agent_tool_calls(&response, &registry).unwrap();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "workspace.write_file");
        assert_eq!(calls[0].arguments["path"], "output/main.md");
    }
}
