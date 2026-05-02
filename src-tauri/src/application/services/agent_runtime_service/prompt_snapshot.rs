use serde_json::{Map, Value, json};

use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::BuiltinAgentToolRegistry;
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole,
};

pub(super) fn request_from_prompt_snapshot(
    prompt_snapshot: &Value,
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let payload = find_payload_object(prompt_snapshot).ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: expected a chat completion payload object".to_string(),
        )
    })?;
    let mut payload = payload.clone();

    payload.insert("stream".to_string(), Value::Bool(false));
    if !payload.contains_key("chat_completion_source") {
        payload.insert(
            "chat_completion_source".to_string(),
            Value::String("openai".to_string()),
        );
    }

    if !payload.contains_key("messages") && !payload.contains_key("prompt") {
        return Err(ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: payload must contain messages or prompt".to_string(),
        ));
    }

    Ok(ChatCompletionGenerateRequestDto { payload })
}

pub(super) fn prepare_agent_tool_request(
    mut request: ChatCompletionGenerateRequestDto,
    registry: &BuiltinAgentToolRegistry,
    run_id: &str,
) -> Result<AgentModelRequest, ApplicationError> {
    reject_external_tool_request(&request.payload)?;

    let mut messages = messages_from_payload(&mut request.payload)?;
    let agent_system_prompt = [
        "TauriTavern Agent Mode is active.",
        "Work through Agent tools. Tool results are private run state, not chat messages.",
        "Use chat_search to find relevant prior messages when you need more context. Only query is required.",
        "Use chat_read_messages with message indexes from chat_search, or exact indexes you already know. For long messages, read smaller ranges with start_char and max_chars.",
        "Use worldinfo_read_activated when active lore for this run matters.",
        "Use workspace_list_files to inspect visible workspace files.",
        "Use workspace_read_file before modifying an existing file. Read output has line numbers; never include line number prefixes in old_string or new_string.",
        "Use workspace_apply_patch for precise edits to existing files. The old_string must match exactly and uniquely unless replace_all is true.",
        "Use workspace_write_file for new files or complete rewrites.",
        "Use persist/ for concise information that should carry into later runs of this same chat, such as durable plot facts, unresolved threads, relationship state, and user style preferences.",
        "Do not copy full chat history, final replies, tool results, or temporary reasoning into persist/.",
        "Write the final chat message body to output/main.md, then call workspace_finish.",
        "Do not answer directly without finishing through workspace_finish.",
    ]
    .join("\n");
    messages.insert(0, text_message(AgentModelRole::System, agent_system_prompt));

    request.payload.remove("tools");
    request.payload.remove("tool_choice");
    request
        .payload
        .insert("stream".to_string(), Value::Bool(false));

    Ok(AgentModelRequest {
        payload: request.payload,
        messages,
        tools: registry.specs().to_vec(),
        tool_choice: Value::String("auto".to_string()),
        provider_state: json!({ "sessionId": run_id }),
    })
}

pub(super) fn reject_external_tool_request(
    payload: &Map<String, Value>,
) -> Result<(), ApplicationError> {
    let has_tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    if has_tools {
        return Err(ApplicationError::ValidationError(
            "agent.external_tools_unsupported_phase2b: Agent Phase 2B owns the tool registry"
                .to_string(),
        ));
    }

    if payload.contains_key("tool_choice") {
        return Err(ApplicationError::ValidationError(
            "agent.external_tool_choice_unsupported_phase2b: Agent Phase 2B owns tool choice"
                .to_string(),
        ));
    }

    if payload
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|message| {
                message
                    .get("role")
                    .and_then(Value::as_str)
                    .is_some_and(|role| role.eq_ignore_ascii_case("tool"))
                    || message
                        .pointer("/tool_calls")
                        .and_then(Value::as_array)
                        .is_some_and(|tool_calls| !tool_calls.is_empty())
            })
        })
    {
        return Err(ApplicationError::ValidationError(
            "agent.external_tool_turns_unsupported_phase2b: prompt snapshot already contains tool turns"
                .to_string(),
        ));
    }

    Ok(())
}

pub(super) fn request_summary(request: &AgentModelRequest) -> Value {
    json!({
        "chatCompletionSource": request.payload.get("chat_completion_source").and_then(Value::as_str),
        "customApiFormat": request.payload.get("custom_api_format").and_then(Value::as_str),
        "model": request.payload.get("model").and_then(Value::as_str),
        "messageCount": request.messages.len(),
        "toolCount": request.tools.len(),
    })
}

fn find_payload_object(value: &Value) -> Option<Map<String, Value>> {
    let object = value.as_object()?;

    for key in [
        "chatCompletionPayload",
        "chat_completion_payload",
        "generateData",
        "generate_data",
    ] {
        if let Some(payload) = object.get(key).and_then(Value::as_object) {
            return Some(payload.clone());
        }
    }

    if object.contains_key("messages") || object.contains_key("prompt") {
        return Some(object.clone());
    }

    None
}

fn messages_from_payload(
    payload: &mut Map<String, Value>,
) -> Result<Vec<AgentModelMessage>, ApplicationError> {
    let messages = match payload.remove("messages") {
        Some(Value::Array(messages)) => messages,
        Some(Value::String(prompt)) => vec![json!({
            "role": "user",
            "content": prompt,
        })],
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "agent.tool_loop_requires_messages: messages must be an array".to_string(),
            ));
        }
        None => {
            let prompt = payload
                .remove("prompt")
                .and_then(|value| value.as_str().map(str::to_string))
                .ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "agent.tool_loop_requires_messages: prompt snapshot must contain messages or a string prompt"
                            .to_string(),
                    )
                })?;
            vec![json!({
                "role": "user",
                "content": prompt,
            })]
        }
    };
    payload.remove("prompt");

    messages
        .iter()
        .map(message_from_openai_value)
        .collect::<Result<Vec<_>, _>>()
}

fn message_from_openai_value(value: &Value) -> Result<AgentModelMessage, ApplicationError> {
    let object = value.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: message must be an object".to_string(),
        )
    })?;
    let role = match object
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .trim()
        .to_lowercase()
        .as_str()
    {
        "system" => AgentModelRole::System,
        "developer" => AgentModelRole::Developer,
        "assistant" => AgentModelRole::Assistant,
        "tool" | "function" => AgentModelRole::Tool,
        _ => AgentModelRole::User,
    };

    let provider_metadata = json!({
        "openai": {
            "name": object.get("name").and_then(Value::as_str),
        }
    });

    Ok(AgentModelMessage {
        role,
        parts: content_parts_from_openai_value(object.get("content")),
        provider_metadata,
    })
}

fn content_parts_from_openai_value(value: Option<&Value>) -> Vec<AgentModelContentPart> {
    match value {
        Some(Value::String(text)) => vec![AgentModelContentPart::Text { text: text.clone() }],
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| match part {
                Value::String(text) => AgentModelContentPart::Text { text: text.clone() },
                Value::Object(object)
                    if object.get("type").and_then(Value::as_str) == Some("text") =>
                {
                    AgentModelContentPart::Text {
                        text: object
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    }
                }
                other => AgentModelContentPart::Native {
                    provider: "openai.content_part".to_string(),
                    value: other.clone(),
                },
            })
            .collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![AgentModelContentPart::Text {
            text: other.to_string(),
        }],
    }
}

fn text_message(role: AgentModelRole, text: String) -> AgentModelMessage {
    AgentModelMessage {
        role,
        parts: vec![AgentModelContentPart::Text { text }],
        provider_metadata: Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{reject_external_tool_request, request_from_prompt_snapshot};

    #[test]
    fn rejects_external_tool_choice_even_when_null() {
        let prompt_snapshot = json!({
            "chatCompletionPayload": {
                "messages": [{ "role": "user", "content": "hello" }],
                "tool_choice": null
            }
        });
        let request = request_from_prompt_snapshot(&prompt_snapshot).expect("request");

        let error = reject_external_tool_request(&request.payload).expect_err("tool_choice fails");
        assert!(
            error
                .to_string()
                .contains("agent.external_tool_choice_unsupported_phase2b")
        );
    }
}
