use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::format::{string_value, usize_value};
use crate::domain::models::agent::{AgentModelMessage, AgentModelRequest, AgentModelRole};

pub(super) const NATIVE_PROVIDER: Option<&str> = Some("openai_responses");
const REASONING_ENCRYPTED_CONTENT: &str = "reasoning.encrypted_content";

pub(super) fn messages_for_request(
    request: &AgentModelRequest,
) -> Result<Vec<&AgentModelMessage>, ApplicationError> {
    if string_value(&request.provider_state, "previousResponseId").is_none() {
        return Ok(request.messages.iter().collect());
    }

    let cursor = usize_value(&request.provider_state, "messageCursor").ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.provider_state_invalid: OpenAI Responses continuation is missing messageCursor"
                .to_string(),
        )
    })?;
    if cursor > request.messages.len() {
        return Err(ApplicationError::ValidationError(format!(
            "agent.provider_state_invalid: messageCursor {cursor} exceeds message count {}",
            request.messages.len()
        )));
    }

    Ok(request
        .messages
        .iter()
        .skip(cursor)
        .filter(|message| message.role != AgentModelRole::Assistant)
        .collect())
}

pub(super) fn apply_payload_overrides(
    payload: &mut Map<String, Value>,
    request: &AgentModelRequest,
) -> Result<(), ApplicationError> {
    if let Some(previous_response_id) = string_value(&request.provider_state, "previousResponseId")
    {
        payload.insert(
            "previous_response_id".to_string(),
            Value::String(previous_response_id.to_string()),
        );
    }

    Ok(())
}

pub(super) fn ensure_reasoning_include(payload: &mut Map<String, Value>) {
    let entry = payload
        .entry("include".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(items) = entry.as_array_mut() else {
        return;
    };
    let encrypted = Value::String(REASONING_ENCRYPTED_CONTENT.to_string());
    if !items.iter().any(|item| item == &encrypted) {
        items.push(encrypted);
    }
}
