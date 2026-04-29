use serde_json::json;

use super::args::{parse_workspace_path, required_trimmed_string_arg};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

use super::super::dispatcher::AgentToolEffect;

pub(in crate::application::services::agent_tools) fn finish(
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let args = call.arguments.as_object();
    let final_path = args
        .and_then(|args| required_trimmed_string_arg(args, "final_path"))
        .unwrap_or("output/main.md");
    let final_path = match parse_workspace_path(call, final_path) {
        Ok(path) => path,
        Err(result) => return Ok((result, AgentToolEffect::None)),
    };

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!("Finished with final artifact {}.", final_path.as_str()),
        structured: json!({
            "finalPath": final_path.as_str(),
            "reason": args.and_then(|args| required_trimmed_string_arg(args, "reason")),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![final_path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::Finish { final_path }))
}
