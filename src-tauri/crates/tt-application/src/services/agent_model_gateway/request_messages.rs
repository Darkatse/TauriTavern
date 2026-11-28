use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::decode::{native_parts, reasoning_content_part};
use crate::services::agent_model_gateway::fields::{
    TOOL_CALL_STRUCTURE_KEYS, message_structure_keys, metadata_with_fields, split_fields,
};
use tt_domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRole, AgentToolResult,
    OPENAI_CONTENT_PART_PROVIDER,
};
use tt_domain::models::tool::{ToolArguments, ToolId, ToolInvocation};

pub(crate) fn canonical_request_messages(
    messages: Vec<Value>,
) -> Result<Vec<AgentModelMessage>, ApplicationError> {
    // Call IDs can repeat across Session runs.
    let mut call_tools = HashMap::<String, ToolId>::new();
    let mut canonical = Vec::with_capacity(messages.len());
    for (index, value) in messages.into_iter().enumerate() {
        let message = if value.get("parts").is_some() {
            serde_json::from_value(value).map_err(|error| invalid(index, error))?
        } else {
            convert_message(index, value, &call_tools)?
        };
        for part in &message.parts {
            if let AgentModelContentPart::ToolCall { call } = part {
                call_tools.insert(call.call_id.clone(), call.tool_id.clone());
            }
        }
        canonical.push(message);
    }
    Ok(canonical)
}

fn convert_message(
    index: usize,
    value: Value,
    call_tools: &HashMap<String, ToolId>,
) -> Result<AgentModelMessage, ApplicationError> {
    let Value::Object(object) = value else {
        return Err(invalid(index, "a message must be an object"));
    };
    let role = AgentModelRole::deserialize(object.get("role").unwrap_or(&Value::Null))
        .map_err(|error| invalid(index, format!("role: {error}")))?;
    let (mut structure, fields) = split_fields(object, message_structure_keys(role));

    let mut parts = match role {
        AgentModelRole::Tool => vec![tool_result(index, &mut structure, call_tools)?],
        _ => content_parts(index, structure.remove("content"))?,
    };
    match structure.remove("reasoning_content") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => parts.push(reasoning_content_part(text)),
        Some(_) => return Err(invalid(index, "reasoning_content must be a string")),
    }
    match structure.remove("native") {
        None | Some(Value::Null) => {}
        Some(Value::Object(native)) => parts.extend(native_parts(native)),
        Some(_) => {
            return Err(invalid(
                index,
                "native must be an object keyed by API format",
            ));
        }
    }
    match structure.remove("tool_calls") {
        None | Some(Value::Null) => {}
        Some(Value::Array(calls)) => {
            for (position, call) in calls.into_iter().enumerate() {
                let call = tool_call(call)
                    .map_err(|reason| invalid(index, format!("tool call {position}: {reason}")))?;
                parts.push(AgentModelContentPart::ToolCall { call });
            }
        }
        Some(_) => return Err(invalid(index, "tool_calls must be an array")),
    }

    Ok(AgentModelMessage {
        role,
        parts,
        provider_metadata: metadata_with_fields(Map::new(), fields),
    })
}

fn content_parts(
    index: usize,
    content: Option<Value>,
) -> Result<Vec<AgentModelContentPart>, ApplicationError> {
    match content {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(vec![AgentModelContentPart::Text { text }]),
        Some(Value::Array(items)) => Ok(items.into_iter().map(content_part).collect()),
        Some(_) => Err(invalid(
            index,
            "content must be text or an array of content parts",
        )),
    }
}

// Preserve extra fields even on text parts.
fn content_part(item: Value) -> AgentModelContentPart {
    if let Value::Object(object) = &item
        && object.len() == 2
        && object.get("type").and_then(Value::as_str) == Some("text")
        && let Some(Value::String(text)) = object.get("text")
    {
        return AgentModelContentPart::Text { text: text.clone() };
    }
    AgentModelContentPart::Native {
        provider: OPENAI_CONTENT_PART_PROVIDER.to_string(),
        value: item,
    }
}

fn tool_result(
    index: usize,
    structure: &mut Map<String, Value>,
    call_tools: &HashMap<String, ToolId>,
) -> Result<AgentModelContentPart, ApplicationError> {
    let call_id = match structure.remove("tool_call_id") {
        Some(Value::String(call_id)) if !call_id.is_empty() => call_id,
        _ => return Err(invalid(index, "a tool message needs a string tool_call_id")),
    };
    let name = structure.remove("name");
    let tool_id = match (call_tools.get(&call_id), name) {
        (Some(tool_id), _) => tool_id.clone(),
        (None, Some(Value::String(name))) => {
            ToolId::prompt(name).map_err(|error| invalid(index, error))?
        }
        (None, _) => {
            return Err(invalid(
                index,
                format!("call `{call_id}` is not made earlier, so the tool message needs a name"),
            ));
        }
    };
    let content = match structure.remove("content") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text,
        Some(_) => return Err(invalid(index, "tool message content must be text")),
    };
    Ok(AgentModelContentPart::ToolResult {
        result: AgentToolResult::new(call_id, tool_id, content, Value::Null),
    })
}

fn tool_call(item: Value) -> Result<ToolInvocation, String> {
    let Value::Object(object) = item else {
        return Err("a tool call must be an object".to_string());
    };
    let (mut structure, fields) = split_fields(object, TOOL_CALL_STRUCTURE_KEYS);
    let call_id = match structure.remove("id") {
        Some(Value::String(call_id)) if !call_id.is_empty() => call_id,
        _ => return Err("a tool call needs a string id".to_string()),
    };
    match structure.remove("type") {
        None => {}
        Some(Value::String(kind)) if kind == "function" => {}
        Some(_) => return Err("only function tool calls are supported".to_string()),
    }
    let Some(Value::Object(mut function)) = structure.remove("function") else {
        return Err("a tool call needs a function object".to_string());
    };
    let Some(Value::String(name)) = function.remove("name") else {
        return Err("a tool call needs a function name".to_string());
    };
    let arguments = ToolArguments::decode(function.remove("arguments").as_ref());
    if let Some(key) = function.keys().next() {
        return Err(format!(
            "a tool call function holds only name and arguments, not `{key}`"
        ));
    }

    Ok(ToolInvocation {
        call_id,
        tool_id: ToolId::prompt(name).map_err(|error| error.to_string())?,
        arguments,
        provider_metadata: metadata_with_fields(Map::new(), fields),
    })
}

fn invalid(index: usize, reason: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.invalid_prompt_messages: message {index}: {reason}"
    ))
}
