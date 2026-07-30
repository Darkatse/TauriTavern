use serde::Serialize;

use crate::errors::ApplicationError;
use tt_domain::models::agent::{AgentToolCall, AgentToolResult};

use super::super::dispatcher::AgentToolEffect;
use super::super::structured::structured_value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceFinishStructured<'a> {
    reason: Option<&'a str>,
}

pub(in crate::services::agent_tools) fn finish(
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let args = call.arguments.as_object();
    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: "Finished the Agent run.".to_string(),
        structured: structured_value(WorkspaceFinishStructured {
            reason: args
                .and_then(|args| args.get("reason"))
                .and_then(serde_json::Value::as_str),
        }),
        is_error: false,
        error_code: None,
        resource_refs: Vec::new(),
    };

    Ok((result, AgentToolEffect::Finish))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::finish;
    use crate::services::agent_tools::AgentToolEffect;
    use tt_domain::models::agent::AgentToolCall;

    #[test]
    fn finish_returns_control_effect() {
        let call = AgentToolCall {
            id: "call_1".to_string(),
            name: "workspace.finish".to_string(),
            arguments: json!({}),
            provider_metadata: json!(null),
        };

        let (_result, effect) = finish(&call).expect("finish");

        assert!(matches!(effect, AgentToolEffect::Finish));
    }
}
