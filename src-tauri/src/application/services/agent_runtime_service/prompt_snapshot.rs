use serde_json::{Map, Value, json};

use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::BuiltinAgentToolRegistry;

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
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let messages = ensure_messages_array(&mut request.payload)?;
    messages.insert(0, json!({
        "role": "system",
        "content": "TauriTavern Agent Mode is active. Use workspace_write_file to write the final answer to output/main.md, then call workspace_finish. Tool results are private run state, not chat messages. Do not answer directly without finishing through tools.",
    }));

    request
        .payload
        .insert("tools".to_string(), Value::Array(registry.openai_tools()));
    request
        .payload
        .insert("tool_choice".to_string(), Value::String("auto".to_string()));
    request
        .payload
        .insert("stream".to_string(), Value::Bool(false));

    Ok(request)
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
            "agent.external_tools_unsupported_phase2a: Agent Phase 2A owns the tool registry"
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
            "agent.external_tool_turns_unsupported_phase2a: prompt snapshot already contains tool turns"
                .to_string(),
        ));
    }

    Ok(())
}

pub(super) fn request_summary(payload: &Map<String, Value>) -> Value {
    json!({
        "chatCompletionSource": payload.get("chat_completion_source").and_then(Value::as_str),
        "model": payload.get("model").and_then(Value::as_str),
        "messageCount": payload.get("messages").and_then(Value::as_array).map(|messages| messages.len()),
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

fn ensure_messages_array(
    payload: &mut Map<String, Value>,
) -> Result<&mut Vec<Value>, ApplicationError> {
    if !payload.contains_key("messages") {
        let prompt = payload
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                ApplicationError::ValidationError(
                    "agent.tool_loop_requires_messages: prompt snapshot must contain messages or a string prompt"
                        .to_string(),
                )
            })?;
        payload.insert(
            "messages".to_string(),
            Value::Array(vec![json!({
                "role": "user",
                "content": prompt,
            })]),
        );
    }

    payload
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.tool_loop_requires_messages: messages must be an array".to_string(),
            )
        })
}
