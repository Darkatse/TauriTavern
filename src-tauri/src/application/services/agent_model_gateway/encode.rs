use serde_json::{Map, Value, json};

use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::format::resolve_request_adapter;
use crate::application::services::agent_model_gateway::provider_state;
use crate::application::services::agent_model_gateway::providers::AgentProviderAdapter;
use crate::application::services::agent_model_gateway::schema;
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole, AgentToolCall,
    AgentToolResult, AgentToolSpec,
};

pub(crate) fn encode_chat_completion_request(
    request: &AgentModelRequest,
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let (_source, adapter) = resolve_request_adapter(request)?;
    let mut payload = request.payload.clone();
    provider_state::apply_provider_state_to_payload(&mut payload, request, adapter)?;

    payload.insert(
        "messages".to_string(),
        Value::Array(
            adapter
                .messages_for_request(request)?
                .into_iter()
                .map(|message| encode_openai_compatible_message(message, &request.tools, adapter))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );

    if !request.tools.is_empty() {
        payload.insert(
            "tools".to_string(),
            Value::Array(schema::render_openai_tools(&request.tools, adapter)),
        );
        payload.insert(
            "tool_choice".to_string(),
            if request.tool_choice.is_null() {
                Value::String("auto".to_string())
            } else {
                request.tool_choice.clone()
            },
        );
    }

    adapter.finalize_payload(&mut payload);
    payload.insert("stream".to_string(), Value::Bool(false));
    Ok(ChatCompletionGenerateRequestDto { payload })
}

fn encode_openai_compatible_message(
    message: &AgentModelMessage,
    tools: &[AgentToolSpec],
    adapter: AgentProviderAdapter,
) -> Result<Value, ApplicationError> {
    let mut object = Map::new();
    object.insert(
        "role".to_string(),
        Value::String(role_name(message.role).to_string()),
    );
    if let Some(name) = message
        .provider_metadata
        .pointer("/openai/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("name".to_string(), Value::String(name.to_string()));
    }

    match message.role {
        AgentModelRole::Assistant => {
            object.insert(
                "content".to_string(),
                openai_content_from_parts(&message.parts),
            );

            let tool_calls = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    AgentModelContentPart::ToolCall { call } => Some(call),
                    _ => None,
                })
                .map(|call| encode_openai_tool_call(call, tools))
                .collect::<Result<Vec<_>, _>>()?;
            if !tool_calls.is_empty() {
                object.insert("tool_calls".to_string(), Value::Array(tool_calls));
            }
        }
        AgentModelRole::Tool => {
            let result = message
                .parts
                .iter()
                .find_map(|part| match part {
                    AgentModelContentPart::ToolResult { result } => Some(result),
                    _ => None,
                })
                .ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "agent.invalid_model_message: tool message is missing tool result"
                            .to_string(),
                    )
                })?;

            object.insert(
                "tool_call_id".to_string(),
                Value::String(result.call_id.clone()),
            );
            object.insert("name".to_string(), Value::String(result.name.clone()));
            object.insert(
                "content".to_string(),
                Value::String(tool_result_message_content(result)?),
            );
        }
        _ => {
            object.insert(
                "content".to_string(),
                openai_content_from_parts(&message.parts),
            );
        }
    }

    copy_native_continuation(&mut object, &message.parts, adapter);
    copy_reasoning_content(&mut object, &message.parts);

    Ok(Value::Object(object))
}

fn role_name(role: AgentModelRole) -> &'static str {
    match role {
        AgentModelRole::System => "system",
        AgentModelRole::Developer => "developer",
        AgentModelRole::User => "user",
        AgentModelRole::Assistant => "assistant",
        AgentModelRole::Tool => "tool",
    }
}

fn openai_content_from_parts(parts: &[AgentModelContentPart]) -> Value {
    let mut text = String::new();
    let mut content_parts = Vec::new();
    let mut needs_array = false;

    for part in parts {
        match part {
            AgentModelContentPart::Text { text: part_text } => {
                if needs_array {
                    content_parts.push(json!({ "type": "text", "text": part_text }));
                } else {
                    text.push_str(part_text);
                }
            }
            AgentModelContentPart::Media { value, .. } => {
                if !text.is_empty() {
                    content_parts.push(json!({ "type": "text", "text": text }));
                    text = String::new();
                }
                needs_array = true;
                content_parts.push(value.clone());
            }
            AgentModelContentPart::Native { provider, value }
                if provider == "openai.content_part" =>
            {
                if !text.is_empty() {
                    content_parts.push(json!({ "type": "text", "text": text }));
                    text = String::new();
                }
                needs_array = true;
                content_parts.push(value.clone());
            }
            _ => {}
        }
    }

    if needs_array {
        if !text.is_empty() {
            content_parts.push(json!({ "type": "text", "text": text }));
        }
        Value::Array(content_parts)
    } else if text.is_empty() {
        Value::Null
    } else {
        Value::String(text)
    }
}

fn copy_native_continuation(
    object: &mut Map<String, Value>,
    parts: &[AgentModelContentPart],
    adapter: AgentProviderAdapter,
) {
    let Some(native_provider) = adapter.native_provider() else {
        return;
    };

    let mut native = Map::new();
    for part in parts {
        if let AgentModelContentPart::Native { provider, value } = part {
            if provider == native_provider {
                native.insert(provider.clone(), value.clone());
            }
        }
    }

    if !native.is_empty() {
        object.insert("native".to_string(), Value::Object(native));
    }
}

fn copy_reasoning_content(object: &mut Map<String, Value>, parts: &[AgentModelContentPart]) {
    let reasoning = parts
        .iter()
        .filter_map(|part| match part {
            AgentModelContentPart::Reasoning { text, .. } => text.as_ref(),
            _ => None,
        })
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if !reasoning.is_empty() {
        object.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning.join("\n\n")),
        );
    }
}

fn encode_openai_tool_call(
    call: &AgentToolCall,
    tools: &[AgentToolSpec],
) -> Result<Value, ApplicationError> {
    let model_name = model_tool_name_for_call(&call.name, tools);
    let arguments = serde_json::to_string(&call.arguments).map_err(|error| {
        ApplicationError::ValidationError(format!("agent.tool_call_serialize_failed: {error}"))
    })?;

    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(call.id.clone()));
    object.insert("type".to_string(), Value::String("function".to_string()));
    object.insert(
        "function".to_string(),
        json!({
            "name": model_name,
            "arguments": arguments,
        }),
    );

    if let Some(signature) = call
        .provider_metadata
        .get("signature")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        object.insert("signature".to_string(), Value::String(signature));
    }

    Ok(Value::Object(object))
}

fn model_tool_name_for_call(name: &str, tools: &[AgentToolSpec]) -> String {
    tools
        .iter()
        .find(|spec| spec.name == name || spec.model_name == name)
        .map(|spec| spec.model_name.clone())
        .unwrap_or_else(|| name.to_string())
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
