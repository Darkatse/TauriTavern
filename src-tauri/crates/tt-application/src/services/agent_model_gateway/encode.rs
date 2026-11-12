use serde_json::{Map, Value, json};
use std::collections::HashMap;

use crate::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::fields;
use crate::services::agent_model_gateway::format::resolve_request_adapter;
use crate::services::agent_model_gateway::provider_state;
use crate::services::agent_model_gateway::providers::AgentProviderAdapter;
use crate::services::agent_model_gateway::schema;
use tt_domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole, AgentModelTool,
    AgentToolResult, OPENAI_CONTENT_PART_PROVIDER,
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

    let mut sent = adapter
        .messages_for_request(request)?
        .into_iter()
        .peekable();
    let mut call_aliases = HashMap::new();
    let mut messages = Vec::new();
    let mut tool_images = Vec::new();
    for message in &request.messages {
        // Record omitted Responses calls too, but never use future calls to name results.
        record_call_aliases(message, &request.tools, &mut call_aliases)?;
        if sent.next_if(|next| std::ptr::eq(*next, message)).is_none() {
            continue;
        }
        if message.role != AgentModelRole::Tool && !tool_images.is_empty() {
            messages.push(tool_image_message(std::mem::take(&mut tool_images)));
        }
        let (encoded, images) =
            encode_openai_compatible_message(message, &request.tools, &call_aliases, adapter)?;
        messages.push(encoded);
        tool_images.extend(images);
    }
    if !tool_images.is_empty() {
        messages.push(tool_image_message(tool_images));
    }
    payload.insert("messages".into(), Value::Array(messages));

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

fn tool_image_message(images: Vec<Value>) -> Value {
    let mut content =
        vec![json!({ "type": "text", "text": "Attached image(s) from tool result:" })];
    content.extend(images);
    json!({ "role": "user", "content": content })
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
    call_aliases: &HashMap<&str, &str>,
    adapter: AgentProviderAdapter,
) -> Result<(Value, Vec<Value>), ApplicationError> {
    let mut object = Map::new();
    let mut tool_images = Vec::new();
    object.insert(
        "role".to_string(),
        Value::String(role_name(message.role).to_string()),
    );

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
            let name = call_aliases
                .get(result.call_id.as_str())
                .copied()
                .or_else(|| tool_model_name(&result.tool_id, tools))
                .ok_or_else(|| {
                    ApplicationError::ValidationError(format!(
                        "agent.tool_result_call_missing: no call for tool result `{}`",
                        result.call_id
                    ))
                })?;
            object.insert("name".to_string(), Value::String(name.to_string()));
            let mut text = tool_result_message_content(result);
            let media = message
                .parts
                .iter()
                .filter_map(|part| match part {
                    AgentModelContentPart::Media { value, .. } => Some(value.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let content = if media.is_empty() {
                Value::String(text)
            } else if adapter == AgentProviderAdapter::OpenAiCompatible {
                tool_images = media;
                if text.is_empty() {
                    text = "(see attached image)".to_string();
                }
                Value::String(text)
            } else {
                let mut parts = vec![json!({ "type": "text", "text": text })];
                parts.extend(media);
                Value::Array(parts)
            };
            object.insert("content".to_string(), content);
        }
        _ => {
            object.insert(
                "content".to_string(),
                openai_content_from_parts(&message.parts),
            );
        }
    }

    copy_native_continuation(&mut object, &message.parts);
    copy_reasoning_content(&mut object, &message.parts);
    fields::merge_fields(
        &mut object,
        &message.provider_metadata,
        fields::message_structure_keys(message.role),
    )?;

    Ok((Value::Object(object), tool_images))
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

enum ContentItem<'a> {
    Text(&'a str),
    Part(&'a Value),
}

fn openai_content_from_parts(parts: &[AgentModelContentPart]) -> Value {
    let items = parts
        .iter()
        .filter_map(|part| match part {
            AgentModelContentPart::Text { text } => Some(ContentItem::Text(text)),
            AgentModelContentPart::Media { value, .. } => Some(ContentItem::Part(value)),
            AgentModelContentPart::Native { provider, value }
                if provider == OPENAI_CONTENT_PART_PROVIDER =>
            {
                Some(ContentItem::Part(value))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    match items.as_slice() {
        [] => Value::Null,
        [ContentItem::Text(text)] => Value::String(text.to_string()),
        _ => Value::Array(
            items
                .into_iter()
                .map(|item| match item {
                    ContentItem::Text(text) => json!({ "type": "text", "text": text }),
                    ContentItem::Part(value) => value.clone(),
                })
                .collect(),
        ),
    }
}

fn copy_native_continuation(object: &mut Map<String, Value>, parts: &[AgentModelContentPart]) {
    let mut native = Map::new();
    for part in parts {
        if let AgentModelContentPart::Native { provider, value } = part
            && provider != OPENAI_CONTENT_PART_PROVIDER
        {
            native.insert(provider.clone(), value.clone());
        }
    }

    if !native.is_empty() {
        object.insert("native".to_string(), Value::Object(native));
    }
}

/// Preserve empty reasoning text: tool-call protocols can require the field.
fn copy_reasoning_content(object: &mut Map<String, Value>, parts: &[AgentModelContentPart]) {
    let reasoning = parts
        .iter()
        .filter_map(|part| match part {
            AgentModelContentPart::Reasoning { text, .. } => text.as_deref(),
            _ => None,
        })
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
    let model_alias = call_model_alias(call, tools)?;
    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(call.call_id.clone()));
    object.insert("type".to_string(), Value::String("function".to_string()));
    object.insert(
        "function".to_string(),
        json!({
            "name": model_alias,
            "arguments": call.arguments.encode_for_replay(),
        }),
    );
    fields::merge_fields(
        &mut object,
        &call.provider_metadata,
        fields::TOOL_CALL_STRUCTURE_KEYS,
    )?;

    Ok(Value::Object(object))
}

fn model_tool_for_id<'a>(
    tool_id: &ToolId,
    tools: &'a [AgentModelTool],
) -> Option<&'a AgentModelTool> {
    tools.iter().find(|tool| tool.tool_id == *tool_id)
}

pub(crate) fn call_model_alias<'a>(
    call: &'a ToolInvocation,
    tools: &'a [AgentModelTool],
) -> Result<&'a str, ApplicationError> {
    call.provider_metadata
        .get("modelAlias")
        .and_then(Value::as_str)
        .or_else(|| tool_model_name(&call.tool_id, tools))
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.tool_history_alias_missing: call `{}` has no recorded model alias",
                call.call_id
            ))
        })
}

fn tool_model_name<'a>(tool_id: &'a ToolId, tools: &'a [AgentModelTool]) -> Option<&'a str> {
    model_tool_for_id(tool_id, tools)
        .map(|tool| tool.model_alias.as_str())
        .or_else(|| tool_id.is_prompt().then_some(tool_id.native_name()))
}

fn record_call_aliases<'a>(
    message: &'a AgentModelMessage,
    tools: &'a [AgentModelTool],
    aliases: &mut HashMap<&'a str, &'a str>,
) -> Result<(), ApplicationError> {
    for part in &message.parts {
        if let AgentModelContentPart::ToolCall { call } = part {
            aliases.insert(call.call_id.as_str(), call_model_alias(call, tools)?);
        }
    }
    Ok(())
}

fn tool_result_message_content(result: &AgentToolResult) -> String {
    if result.is_error {
        format!("## Tool error\n\n{}", result.content.trim())
    } else {
        result.content.clone()
    }
}
