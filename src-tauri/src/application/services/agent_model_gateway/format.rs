use serde_json::Value;

use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::providers::AgentProviderAdapter;
use crate::application::services::chat_completion_service::exchange::ChatCompletionProviderFormat;
use crate::domain::models::agent::AgentModelRequest;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

pub(super) fn resolve_request_adapter(
    request: &AgentModelRequest,
) -> Result<(ChatCompletionSource, AgentProviderAdapter), ApplicationError> {
    let source = ChatCompletionSource::parse(
        request
            .payload
            .get("chat_completion_source")
            .and_then(Value::as_str)
            .unwrap_or("openai"),
    )
    .ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.model_request_invalid_source: unsupported chat completion source".to_string(),
        )
    })?;
    let format = ChatCompletionProviderFormat::from_payload(source, &request.payload)?;

    Ok((source, AgentProviderAdapter::from_format(format)))
}

pub(super) fn string_value<'a>(state: &'a Value, key: &str) -> Option<&'a str> {
    state
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn usize_value(state: &Value, key: &str) -> Option<usize> {
    state
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}
