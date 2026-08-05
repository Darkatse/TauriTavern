use serde::Serialize;

use crate::errors::ApplicationError;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;

use super::super::dispatcher::AgentToolEffect;
use super::super::structured::structured_value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceFinishStructured<'a> {
    reason: Option<&'a str>,
}

pub(in crate::services::agent_tools) fn finish(
    call: &ToolInvocation,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let args = call.arguments.as_object();
    let result = AgentToolResult {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
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
    use tt_domain::models::tool::{ToolId, ToolInvocation};

    #[test]
    fn finish_returns_control_effect() {
        let call = ToolInvocation {
            call_id: "call_1".to_string(),
            tool_id: ToolId::builtin("workspace.finish").unwrap(),
            arguments: json!({}),
            provider_metadata: json!(null),
        };

        let (_result, effect) = finish(&call).expect("finish");

        assert!(matches!(effect, AgentToolEffect::Finish));
    }
}
