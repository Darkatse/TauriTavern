use serde_json::{Map, Value, json};

use crate::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::format::resolve_request_adapter;
use crate::services::agent_model_gateway::provider_state;
use crate::services::agent_model_gateway::providers::AgentProviderAdapter;
use crate::services::agent_model_gateway::schema;
use tt_domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole, AgentModelTool,
    AgentToolResult,
};
use tt_domain::models::tool::{ToolChoice, ToolId, ToolInvocation};

pub(crate) fn encode_chat_completion_request(
    request: &AgentModelRequest,
    stream: bool,
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let (_source, adapter) = resolve_request_adapter(request)?;
    let mut payload = request.payload.clone();
    provider_state::apply_provider_state_to_payload(&mut payload, request, adapter)?;
    payload.remove("tools");
    payload.remove("tool_choice");

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

    if request.tools.is_empty() {
        if matches!(
            request.tool_choice,
            ToolChoice::Required | ToolChoice::Specific(_)
        ) {
            return Err(ApplicationError::ValidationError(
                "agent.tool_choice_requires_tools: required and specific tool choice need at least one advertised tool"
                    .to_string(),
            ));
        }
    } else {
        payload.insert(
            "tools".to_string(),
            Value::Array(schema::render_openai_tools(&request.tools, adapter)),
        );
        payload.insert(
            "tool_choice".to_string(),
            encode_tool_choice(&request.tool_choice, &request.tools)?,
        );
    }

    adapter.finalize_payload(&mut payload);
    payload.insert("stream".to_string(), Value::Bool(stream));
    payload.insert("n".to_string(), json!(1));
    Ok(ChatCompletionGenerateRequestDto { payload })
}

fn encode_tool_choice(
    choice: &ToolChoice,
    tools: &[AgentModelTool],
) -> Result<Value, ApplicationError> {
    match choice {
        ToolChoice::None => Ok(Value::String("none".to_string())),
        ToolChoice::Auto => Ok(Value::String("auto".to_string())),
        ToolChoice::Required => Ok(Value::String("required".to_string())),
        ToolChoice::Specific(tool_id) => {
            let tool = model_tool_for_id(tool_id, tools).ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.tool_choice_tool_not_advertised: tool `{tool_id}` is not advertised in this request"
                ))
            })?;

            Ok(json!({
                "type": "function",
                "function": { "name": tool.model_alias },
            }))
        }
    }
}

fn encode_openai_compatible_message(
    message: &AgentModelMessage,
    tools: &[AgentModelTool],
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
            object.insert(
                "name".to_string(),
                Value::String(
                    model_tool_for_id(&result.tool_id, tools)
                        .ok_or_else(|| tool_history_not_advertised(&result.tool_id))?
                        .model_alias
                        .clone(),
                ),
            );
            object.insert(
                "content".to_string(),
                Value::String(tool_result_message_content(result)),
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
        if let AgentModelContentPart::Native { provider, value } = part
            && provider == native_provider
        {
            native.insert(provider.clone(), value.clone());
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
    call: &ToolInvocation,
    tools: &[AgentModelTool],
) -> Result<Value, ApplicationError> {
    let model_alias = model_tool_for_id(&call.tool_id, tools)
        .map(|tool| tool.model_alias.as_str())
        .ok_or_else(|| tool_history_not_advertised(&call.tool_id))?;
    let arguments = serde_json::to_string(&call.arguments).map_err(|error| {
        ApplicationError::ValidationError(format!("agent.tool_call_serialize_failed: {error}"))
    })?;

    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(call.call_id.clone()));
    object.insert("type".to_string(), Value::String("function".to_string()));
    object.insert(
        "function".to_string(),
        json!({
            "name": model_alias,
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

    if let Some(extra_content) = call.provider_metadata.pointer("/raw/extra_content") {
        object.insert("extra_content".to_string(), extra_content.clone());
    }

    Ok(Value::Object(object))
}

fn model_tool_for_id<'a>(
    tool_id: &ToolId,
    tools: &'a [AgentModelTool],
) -> Option<&'a AgentModelTool> {
    tools.iter().find(|tool| tool.tool_id == *tool_id)
}

fn tool_history_not_advertised(tool_id: &ToolId) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.tool_history_not_advertised: tool `{tool_id}` is not advertised in this request"
    ))
}

fn tool_result_message_content(result: &AgentToolResult) -> String {
    if result.is_error {
        format!("## Tool error\n\n{}", result.content.trim())
    } else {
        result.content.clone()
    }
}
