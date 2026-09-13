use serde::Serialize;
use serde_json::{Map, Value};

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
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let result = AgentToolResult {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        content: "Finished the Agent run.".to_string(),
        structured: structured_value(WorkspaceFinishStructured {
            reason: args.get("reason").and_then(Value::as_str),
        }),
        is_error: false,
        error_code: None,
        resource_refs: Vec::new(),
    };

    Ok((result, AgentToolEffect::Finish))
}
