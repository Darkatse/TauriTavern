use serde_json::{Map, Value, json};

use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::format::string_value;
use crate::application::services::agent_model_gateway::providers::AgentProviderAdapter;
use crate::domain::models::agent::{AgentModelContentPart, AgentModelRequest, AgentModelResponse};
use crate::domain::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionSource,
};

pub(super) fn apply_provider_state_to_payload(
    payload: &mut Map<String, Value>,
    request: &AgentModelRequest,
    adapter: AgentProviderAdapter,
) -> Result<(), ApplicationError> {
    if request.provider_state.is_null() {
        return Ok(());
    }

    payload.insert(
        CHAT_COMPLETION_PROVIDER_STATE_FIELD.to_string(),
        request.provider_state.clone(),
    );
    adapter.apply_payload_overrides(payload, request)
}

pub(super) fn next_provider_state(
    request: &AgentModelRequest,
    source: ChatCompletionSource,
    adapter: AgentProviderAdapter,
    response: &AgentModelResponse,
) -> Result<Value, ApplicationError> {
    let session_id = string_value(&request.provider_state, "sessionId")
        .map(str::to_string)
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.provider_state_invalid: sessionId is required".to_string(),
            )
        })?;

    let response_id = response
        .provider_metadata
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let mut state = json!({
        "sessionId": session_id,
        "chatCompletionSource": source.key(),
        "providerFormat": adapter.format().key(),
        "messageCursor": request.messages.len(),
        "lastResponseId": response_id.clone(),
    });

    if let Some(provider) = adapter.native_provider() {
        let part_count = native_part_count(response, provider);
        if !response.tool_calls.is_empty() && part_count == 0 {
            return Err(ApplicationError::ValidationError(format!(
                "model.native_metadata_lost: {provider} continuation requires native metadata"
            )));
        }
        state["nativeContinuation"] = json!({
            "provider": provider,
            "partCount": part_count,
        });
    }

    if adapter == AgentProviderAdapter::OpenAiResponses {
        let response_id = response_id.ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.provider_state_invalid: OpenAI Responses continuation is missing response id"
                    .to_string(),
            )
        })?;
        state["transport"] = Value::String("responses_websocket".to_string());
        state["previousResponseId"] = Value::String(response_id);
    }

    Ok(state)
}

fn native_part_count(response: &AgentModelResponse, provider: &str) -> usize {
    response
        .message
        .parts
        .iter()
        .filter(|part| {
            matches!(
                part,
                AgentModelContentPart::Native {
                    provider: part_provider,
                    ..
                } if part_provider == provider
            )
        })
        .count()
}
