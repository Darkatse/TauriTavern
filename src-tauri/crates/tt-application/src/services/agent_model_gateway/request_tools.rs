use std::collections::HashSet;

use serde_json::Value;

use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::fields::{TOOL_FUNCTION_STRUCTURE_KEYS, split_fields};
use tt_domain::models::agent::AgentModelTool;
use tt_domain::models::tool::ToolId;

pub(crate) fn canonical_request_tools(
    tools: Option<Value>,
) -> Result<Vec<AgentModelTool>, ApplicationError> {
    let tools = match tools {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(tools)) => tools,
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "agent.invalid_prompt_tools: tools must be an array".to_string(),
            ));
        }
    };
    let mut names = HashSet::with_capacity(tools.len());
    tools
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let tool = convert_tool(value).map_err(|reason| invalid(index, reason))?;
            if !names.insert(tool.model_alias.clone()) {
                return Err(invalid(
                    index,
                    format!("`{}` is defined more than once", tool.model_alias),
                ));
            }
            Ok(tool)
        })
        .collect()
}

fn convert_tool(value: Value) -> Result<AgentModelTool, String> {
    let Value::Object(mut object) = value else {
        return Err("a tool definition must be an object".to_string());
    };
    match object.remove("type") {
        None => {}
        Some(Value::String(kind)) if kind == "function" => {}
        Some(_) => return Err("only function tools are supported".to_string()),
    }
    let Some(Value::Object(function)) = object.remove("function") else {
        return Err("a tool definition needs a function object".to_string());
    };
    if let Some(key) = object.keys().next() {
        return Err(format!(
            "a tool definition holds only type and function, not `{key}`"
        ));
    }

    let (mut structure, fields) = split_fields(function, TOOL_FUNCTION_STRUCTURE_KEYS);
    let Some(Value::String(name)) = structure.remove("name") else {
        return Err("a tool definition needs a function name".to_string());
    };
    let description = match structure.remove("description") {
        None | Some(Value::Null) => None,
        Some(Value::String(description)) => Some(description),
        Some(_) => return Err("description must be a string".to_string()),
    };
    Ok(AgentModelTool {
        tool_id: ToolId::prompt(&name).map_err(|error| error.to_string())?,
        model_alias: name,
        description,
        input_schema: structure.remove("parameters").unwrap_or(Value::Null),
        fields,
    })
}

fn invalid(index: usize, reason: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.invalid_prompt_tools: tool {index}: {reason}"
    ))
}
