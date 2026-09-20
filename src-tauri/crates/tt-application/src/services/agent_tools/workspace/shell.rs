use std::sync::Arc;

use serde_json::{Map, Value, json};
use tokio::sync::watch;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::tool::ToolInvocation;
use tt_ports::workspace_shell::{
    WorkspaceShell, WorkspaceShellExit, WorkspaceShellRequest, WorkspaceShellResult,
};

use super::args::{ensure_only_args, required_raw_string_arg, tool_error};
use crate::errors::ApplicationError;
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use crate::services::agent_workspace_scope::ScopedWorkspaceFs;

pub(in crate::services::agent_tools) async fn shell(
    engine: &dyn WorkspaceShell,
    workspace: Arc<ScopedWorkspaceFs>,
    call: &ToolInvocation,
    args: &Map<String, Value>,
    cancel: watch::Receiver<bool>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let invalid = |message: &str| {
        Ok((
            tool_error(call, "tool.invalid_arguments", message),
            AgentToolEffect::None,
        ))
    };
    if let Err(message) = ensure_only_args(args, &["command", "workdir"]) {
        return invalid(&message);
    }
    let Some(command) = required_raw_string_arg(args, "command") else {
        return invalid("command must be a string");
    };
    let workdir = match args.get("workdir") {
        None => "/",
        Some(Value::String(path)) => path,
        Some(_) => return invalid("workdir must be a string"),
    };
    let output = engine
        .execute(WorkspaceShellRequest {
            command: command.to_string(),
            workdir: workdir.to_string(),
            files: workspace.clone(),
            cancel,
        })
        .await?;
    let (exit_code, error_code) = match output.exit {
        WorkspaceShellExit::Exited(0) => (Some(0), None),
        WorkspaceShellExit::Exited(code) => (Some(code), Some("workspace.shell_exit")),
        WorkspaceShellExit::Cancelled => (None, Some("workspace.shell_cancelled")),
        WorkspaceShellExit::TimedOut => (None, Some("workspace.shell_timeout")),
        WorkspaceShellExit::Failed => (None, Some("workspace.shell_failed")),
    };
    let result = AgentToolResult {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        content: render_output(&output),
        structured: json!({"exitCode": exit_code, "outputTruncated": output.output_truncated}),
        is_error: error_code.is_some(),
        error_code: error_code.map(str::to_string),
        resource_refs: Vec::new(),
    };
    let effect = if result.is_error {
        AgentToolEffect::AutoCommitCandidateUpdated { path: None }
    } else if let Some(mutation) = workspace
        .text_mutation()
        .filter(|mutation| mutation.changed)
    {
        AgentToolEffect::AutoCommitCandidateUpdated {
            path: mutation.candidate,
        }
    } else {
        AgentToolEffect::None
    };
    Ok((result, effect))
}

fn render_output(output: &WorkspaceShellResult) -> String {
    let mut content = match output.exit {
        WorkspaceShellExit::Exited(code) => format!("Exit code: {code}"),
        WorkspaceShellExit::Cancelled => "Command cancelled.".to_string(),
        WorkspaceShellExit::TimedOut => "Command timed out.".to_string(),
        WorkspaceShellExit::Failed => String::new(),
    };
    for (label, text) in [("", &output.stdout), ("stderr:\n", &output.stderr)] {
        if text.is_empty() {
            continue;
        }
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        if !output.stdout.is_empty() {
            content.push_str(label);
        }
        content.push_str(text);
    }
    if output.output_truncated {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("Output truncated.");
    }
    content
}
