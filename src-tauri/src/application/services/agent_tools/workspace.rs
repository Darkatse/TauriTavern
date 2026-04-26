use serde_json::{Map, Value, json};

use super::dispatcher::AgentToolEffect;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult, AgentToolSpec, WorkspacePath};
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

pub(super) const WORKSPACE_WRITE_FILE: &str = "workspace.write_file";
pub(super) const WORKSPACE_FINISH: &str = "workspace.finish";

const MODEL_WORKSPACE_WRITE_FILE: &str = "workspace_write_file";
const MODEL_WORKSPACE_FINISH: &str = "workspace_finish";

pub(super) async fn write_file(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = call.arguments.as_object() else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let Some(path) = string_arg(args, "path") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "path is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(content) = string_arg(args, "content") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "content is required"),
            AgentToolEffect::None,
        ));
    };

    let path = WorkspacePath::parse(path)?;
    ensure_writable_workspace_path(&path)?;
    let file = workspace_repository
        .write_text(run_id, &path, content)
        .await?;

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!("Wrote {} bytes to {}.", file.bytes, file.path.as_str()),
        structured: json!({
            "path": file.path.as_str(),
            "bytes": file.bytes,
            "sha256": file.sha256.as_str(),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![file.path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::WorkspaceFileWritten { file }))
}

pub(super) fn finish(
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let args = call.arguments.as_object();
    let final_path = args
        .and_then(|args| string_arg(args, "final_path"))
        .unwrap_or("output/main.md");
    let final_path = WorkspacePath::parse(final_path)?;

    let result = AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!("Finished with final artifact {}.", final_path.as_str()),
        structured: json!({
            "finalPath": final_path.as_str(),
            "reason": args.and_then(|args| string_arg(args, "reason")),
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![final_path.as_str().to_string()],
    };

    Ok((result, AgentToolEffect::Finish { final_path }))
}

pub(super) fn workspace_write_file_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_WRITE_FILE.to_string(),
        model_name: MODEL_WORKSPACE_WRITE_FILE.to_string(),
        title: "Workspace Write File".to_string(),
        description: "Write UTF-8 text to a writable Agent workspace file. Use output/main.md for the final chat message body, then call workspace_finish.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative workspace path. Phase 2A writable prefixes are output/, scratch/, plan/, and summaries/."
                },
                "content": {
                    "type": "string",
                    "description": "Complete UTF-8 file content."
                }
            },
            "required": ["path", "content"]
        }),
        output_schema: None,
        annotations: json!({ "mutating": true }),
        source: "builtin".to_string(),
    }
}

pub(super) fn workspace_finish_spec() -> AgentToolSpec {
    AgentToolSpec {
        name: WORKSPACE_FINISH.to_string(),
        model_name: MODEL_WORKSPACE_FINISH.to_string(),
        title: "Workspace Finish".to_string(),
        description: "Finish the Agent loop after the final artifact has been written. The default final_path is output/main.md.".to_string(),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "final_path": {
                    "type": "string",
                    "description": "Relative workspace path for the final artifact. Defaults to output/main.md."
                },
                "reason": {
                    "type": "string",
                    "description": "Short completion reason."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "control": true }),
        source: "builtin".to_string(),
    }
}

fn ensure_writable_workspace_path(path: &WorkspacePath) -> Result<(), ApplicationError> {
    let value = path.as_str();
    if value.starts_with("output/")
        || value.starts_with("scratch/")
        || value.starts_with("plan/")
        || value.starts_with("summaries/")
    {
        return Ok(());
    }

    Err(ApplicationError::PermissionDenied(format!(
        "agent.workspace_write_denied: path `{value}` is not writable in Phase 2A"
    )))
}

fn string_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn tool_error(call: &AgentToolCall, error_code: &str, message: &str) -> AgentToolResult {
    AgentToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: message.to_string(),
        structured: json!({
            "error": {
                "code": error_code,
                "message": message,
            }
        }),
        is_error: true,
        error_code: Some(error_code.to_string()),
        resource_refs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_policy_rejects_input_paths() {
        let path = WorkspacePath::parse("input/prompt_snapshot.json").unwrap();
        assert!(ensure_writable_workspace_path(&path).is_err());
    }
}
