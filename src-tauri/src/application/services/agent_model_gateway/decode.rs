use serde_json::{Map, Value, json};

use crate::application::errors::ApplicationError;
use crate::application::services::chat_completion_service::exchange::{
    ChatCompletionExchange, NormalizedChatCompletionResponse,
};
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelResponse, AgentModelRole, AgentToolCall,
    AgentToolSpec,
};

#[cfg(test)]
pub(crate) fn decode_chat_completion_response(
    response: Value,
    tools: &[AgentToolSpec],
) -> Result<AgentModelResponse, ApplicationError> {
    let normalized = NormalizedChatCompletionResponse::from_value(response)?;
    decode_normalized_chat_completion_response(&normalized, tools)
}

pub(super) fn decode_chat_completion_exchange(
    exchange: ChatCompletionExchange,
    tools: &[AgentToolSpec],
) -> Result<AgentModelResponse, ApplicationError> {
    if !exchange
        .normalization_report
        .synthetic_tool_call_ids()
        .is_empty()
    {
        return Err(ApplicationError::ValidationError(format!(
            "model.invalid_tool_call: provider response is missing tool_call_id for tool calls: {}",
            exchange
                .normalization_report
                .synthetic_tool_call_ids()
                .join(", ")
        )));
    }

    let mut response =
        decode_normalized_chat_completion_response(&exchange.normalized_response, tools)?;
    let provider_metadata = response.provider_metadata.clone();
    response.provider_metadata = json!({
        "id": provider_metadata.get("id"),
        "model": provider_metadata.get("model"),
        "usage": provider_metadata.get("usage"),
        "chatCompletionSource": exchange.source.key(),
        "providerFormat": exchange.provider_format.key(),
    });
    Ok(response)
}

fn decode_normalized_chat_completion_response(
    response: &NormalizedChatCompletionResponse,
    tools: &[AgentToolSpec],
) -> Result<AgentModelResponse, ApplicationError> {
    let message = response.assistant_message();
    let raw_response = response.raw();

    let text = extract_text_from_message(message);
    let tool_calls = extract_tool_calls_from_message(message, tools)?;
    let mut parts = Vec::new();

    if !text.trim().is_empty() {
        parts.push(AgentModelContentPart::Text { text: text.clone() });
    }

    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(AgentModelContentPart::Reasoning {
            text: Some(reasoning),
            provider_metadata: json!({ "source": "reasoning_content" }),
        });
    }

    if let Some(native) = message.get("native").and_then(Value::as_object) {
        for (provider, value) in native {
            parts.push(AgentModelContentPart::Native {
                provider: provider.clone(),
                value: value.clone(),
            });
        }
    }

    for call in &tool_calls {
        parts.push(AgentModelContentPart::ToolCall { call: call.clone() });
    }

    let model_message = AgentModelMessage {
        role: AgentModelRole::Assistant,
        parts,
        provider_metadata: json!({
            "message": message,
            "responseId": raw_response.get("id"),
            "model": raw_response.get("model"),
        }),
    };

    Ok(AgentModelResponse {
        message: model_message,
        tool_calls,
        text,
        provider_metadata: json!({
            "id": raw_response.get("id"),
            "model": raw_response.get("model"),
            "usage": raw_response.get("usage"),
        }),
        raw_response: raw_response.clone(),
    })
}

fn extract_tool_calls_from_message(
    message: &Map<String, Value>,
    tools: &[AgentToolSpec],
) -> Result<Vec<AgentToolCall>, ApplicationError> {
    let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    calls
        .iter()
        .map(|call| parse_tool_call(call, tools))
        .collect()
}

fn parse_tool_call(
    call: &Value,
    tools: &[AgentToolSpec],
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
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "model.invalid_tool_call: tool_call_id is required".to_string(),
            )
        })?;
    let canonical_name = canonical_tool_name(raw_name, tools).unwrap_or(raw_name);
    let arguments =
        parse_tool_call_arguments(function.get("arguments").or_else(|| function.get("args")));

    Ok(AgentToolCall {
        id: id.to_string(),
        name: canonical_name.to_string(),
        arguments,
        provider_metadata: json!({
            "modelName": raw_name,
            "signature": object.get("signature"),
            "raw": call,
        }),
    })
}

fn canonical_tool_name<'a>(raw: &'a str, tools: &'a [AgentToolSpec]) -> Option<&'a str> {
    tools
        .iter()
        .find(|spec| spec.model_name == raw || spec.name == raw)
        .map(|spec| spec.name.as_str())
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

fn extract_text_from_message(message: &Map<String, Value>) -> String {
    text_from_value(message.get("content")).unwrap_or_default()
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
