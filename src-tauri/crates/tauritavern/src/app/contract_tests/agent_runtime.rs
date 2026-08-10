use super::*;
use tt_domain::models::agent::{
    AgentDelegationContinuation, AgentInvocationExitPolicy, AgentInvocationKind,
    AgentInvocationStatus, AgentModelRole, AgentTaskStatus, ROOT_AGENT_INVOCATION_ID,
};

mod delegation;
mod execution;
mod mcp;

fn allow_profile_tool(allow: &mut Vec<String>, name: &str) {
    let id = format!("builtin:{name}");
    if !allow.iter().any(|tool| tool == &id) {
        allow.push(id);
    }
}

fn model_tool_response(tool_calls: Vec<Value>) -> Value {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": tool_calls,
            }
        }]
    })
}

fn model_tool_call(id: &str, name: &str, arguments: Value) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": serde_json::to_string(&arguments).expect("serialize tool arguments"),
        }
    })
}

fn message_text_for_role(request: &AgentModelRequest, role: AgentModelRole) -> &str {
    request
        .messages
        .iter()
        .find(|message| message.role == role)
        .and_then(|message| {
            message.parts.iter().find_map(|part| match part {
                AgentModelContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
        })
        .expect("message text for role")
}
