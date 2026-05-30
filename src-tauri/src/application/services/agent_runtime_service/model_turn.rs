use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelResponse,
    AgentModelRole, AgentToolResult,
};

pub(super) fn assistant_message_for_next_turn(
    response: &AgentModelResponse,
) -> Result<AgentModelMessage, ApplicationError> {
    if response.tool_calls.is_empty() {
        return Err(ApplicationError::ValidationError(
            "model.invalid_tool_response: assistant message is missing tool calls".to_string(),
        ));
    }

    Ok(response.message.clone())
}

pub(super) fn append_tool_turn_to_request(
    request: &mut AgentModelRequest,
    assistant_message: AgentModelMessage,
    tool_results: &[AgentToolResult],
) -> Result<(), ApplicationError> {
    request.messages.push(assistant_message);
    request.messages.extend(
        tool_results
            .iter()
            .cloned()
            .map(|result| AgentModelMessage {
                role: AgentModelRole::Tool,
                parts: vec![AgentModelContentPart::ToolResult { result }],
                provider_metadata: serde_json::Value::Null,
            }),
    );
    Ok(())
}

pub(super) fn extract_response_text(response: &AgentModelResponse) -> &str {
    response.text.as_str()
}
