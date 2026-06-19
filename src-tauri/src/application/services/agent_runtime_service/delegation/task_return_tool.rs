use std::time::Instant;

use serde_json::{Map, Value, json};

use super::rendering::render_task_return_summary;
use super::task_status::{task_is_terminal, task_return_status, task_status_label};
use super::tool_error::tool_error_outcome;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::application::services::agent_workspace_scope::{
    format_model_workspace_roots, task_result_summary_path, workspace_path_is_under_any_root,
};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentInvocationExitPolicy, AgentRunEventLevel, AgentTaskRecord, AgentToolCall, AgentToolResult,
    WorkspacePath,
};

impl AgentRuntimeService {
    pub(in crate::application::services::agent_runtime_service) async fn dispatch_task_return_tool(
        &self,
        run_id: &str,
        invocation_id: &str,
        call: &AgentToolCall,
        exit_policy: AgentInvocationExitPolicy,
        profile: &ResolvedAgentProfile,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        if exit_policy != AgentInvocationExitPolicy::TaskReturnRequired {
            return Ok(tool_error_outcome(
                call,
                "agent.task_return_denied",
                "task.return is available only while completing a delegated task.",
                started.elapsed().as_millis(),
            ));
        }
        let Some(args) = call.arguments.as_object() else {
            return Ok(tool_error_outcome(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
                started.elapsed().as_millis(),
            ));
        };
        let summary = match required_trimmed_string(args, "summary") {
            Ok(summary) => summary,
            Err(message) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
        };
        let status = match task_return_status(args.get("status")) {
            Ok(status) => status,
            Err(message) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
        };
        let task = self
            .task_for_child_invocation(run_id, invocation_id)
            .await?
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.task_record_missing: no task record owns child invocation `{invocation_id}`"
                ))
            })?;
        if task_is_terminal(task.status) {
            return Ok(tool_error_outcome(
                call,
                "agent.task_already_finished",
                "This delegated task is already finished and cannot accept another task.return.",
                started.elapsed().as_millis(),
            ));
        }
        let result_ref = WorkspacePath::parse(format!("agent-results/{invocation_id}.json"))?;
        let summary_ref = task_result_summary_path(&task.workspace_key)?;
        let result_payload = match normalize_task_return_arguments(&call.arguments, profile) {
            Ok(arguments) => arguments,
            Err(error) => {
                return Ok(tool_error_outcome(
                    call,
                    error.code,
                    &error.message,
                    started.elapsed().as_millis(),
                ));
            }
        };
        let result_doc = json!({
            "schemaVersion": 1,
            "kind": "tauritavern.agentTaskResult",
            "status": status,
            "summary": summary,
            "summaryRef": summary_ref.as_str(),
            "result": result_payload,
            "runtime": {
                "taskId": task.id.as_str(),
                "runId": run_id,
                "parentInvocationId": task.parent_invocation_id.as_str(),
                "childInvocationId": invocation_id,
                "targetProfileId": task.target_profile_id.as_str(),
                "workspaceKey": task.workspace_key.as_str(),
            },
        });
        let result_text = serde_json::to_string_pretty(&result_doc).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.task_return_result_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(run_id, &result_ref, &result_text)
            .await?;
        self.workspace_repository
            .write_text(
                run_id,
                &summary_ref,
                &render_task_return_summary(&result_doc),
            )
            .await?;

        let transition = self
            .transition_child_task_with_change(
                run_id,
                &task.id,
                status,
                Some(result_ref.as_str().to_string()),
                None,
            )
            .await?;
        if !transition.changed {
            return Ok(tool_error_outcome(
                call,
                "agent.task_already_finished",
                "This delegated task is already finished and cannot accept another task.return.",
                started.elapsed().as_millis(),
            ));
        }
        let task = transition.task;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "task_return_completed",
            json!({
                "taskId": task.id.as_str(),
                "parentInvocationId": task.parent_invocation_id.as_str(),
                "childInvocationId": invocation_id,
                "status": task.status,
                "resultRef": result_ref.as_str(),
                "summaryRef": summary_ref.as_str(),
            }),
        )
        .await?;

        Ok(AgentToolDispatchOutcome {
            result: AgentToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: format!(
                    "Returned {} result for delegated task {}.",
                    task_status_label(status),
                    task.id
                ),
                structured: result_doc,
                is_error: false,
                error_code: None,
                resource_refs: vec![
                    result_ref.as_str().to_string(),
                    summary_ref.as_str().to_string(),
                ],
            },
            effect: AgentToolEffect::TaskReturned {
                status,
                result_ref,
                summary,
            },
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    async fn task_for_child_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<Option<AgentTaskRecord>, ApplicationError> {
        let mut matches = self
            .invocation_repository
            .list_tasks(run_id)
            .await?
            .into_iter()
            .filter(|task| task.child_invocation_id == invocation_id)
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(ApplicationError::ValidationError(format!(
                "agent.duplicate_task_record: multiple tasks own child invocation `{invocation_id}`"
            )));
        }
        Ok(matches.pop())
    }
}

fn required_trimmed_string(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{key} must be a non-empty string"))
}

struct TaskReturnArgumentError {
    code: &'static str,
    message: String,
}

impl TaskReturnArgumentError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

fn normalize_task_return_arguments(
    arguments: &Value,
    profile: &ResolvedAgentProfile,
) -> Result<Value, TaskReturnArgumentError> {
    let Some(args) = arguments.as_object() else {
        return Ok(arguments.clone());
    };
    let mut args = args.clone();
    let Some(artifacts_value) = args.get("artifacts") else {
        return Ok(Value::Object(args));
    };
    let Some(artifacts) = artifacts_value.as_array() else {
        return Err(TaskReturnArgumentError::new(
            "tool.invalid_arguments",
            "artifacts must be an array",
        ));
    };

    let mut normalized_artifacts = Vec::with_capacity(artifacts.len());
    for (index, artifact) in artifacts.iter().enumerate() {
        let Some(artifact_object) = artifact.as_object() else {
            return Err(TaskReturnArgumentError::new(
                "tool.invalid_arguments",
                format!("artifacts[{index}] must be an object"),
            ));
        };
        let mut artifact_object = artifact_object.clone();
        let Some(path) = artifact_object.get("path").and_then(Value::as_str) else {
            return Err(TaskReturnArgumentError::new(
                "tool.invalid_arguments",
                format!("artifacts[{index}].path must be a workspace path string"),
            ));
        };
        let path = WorkspacePath::parse(path).map_err(|error| {
            TaskReturnArgumentError::new(
                "workspace.invalid_path",
                format!("artifacts[{index}].path is not a valid workspace path: {error}"),
            )
        })?;
        if !workspace_path_is_under_any_root(&path, &profile.workspace.visible_roots) {
            return Err(TaskReturnArgumentError::new(
                "workspace.path_not_visible",
                format!(
                    "artifacts[{index}].path `{}` is not visible to this delegated task. Regenerate task_return with artifact paths under visible roots: {}.",
                    path.as_str(),
                    format_model_workspace_roots(&profile.workspace.visible_roots)
                ),
            ));
        }
        artifact_object.insert("path".to_string(), Value::String(path.as_str().to_string()));
        normalized_artifacts.push(Value::Object(artifact_object));
    }
    args.insert("artifacts".to_string(), Value::Array(normalized_artifacts));
    Ok(Value::Object(args))
}
