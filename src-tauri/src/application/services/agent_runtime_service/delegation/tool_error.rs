use serde_json::json;

use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

pub(super) fn tool_error_outcome(
    call: &AgentToolCall,
    code: &str,
    message: &str,
    elapsed_ms: u128,
) -> AgentToolDispatchOutcome {
    AgentToolDispatchOutcome {
        result: AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content: message.to_string(),
            structured: json!({
                "error": {
                    "code": code,
                    "message": message,
                }
            }),
            is_error: true,
            error_code: Some(code.to_string()),
            resource_refs: Vec::new(),
        },
        effect: AgentToolEffect::None,
        elapsed_ms,
    }
}
