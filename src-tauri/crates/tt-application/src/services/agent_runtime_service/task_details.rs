use serde::Deserialize;
use serde_json::{Map, Value};

use super::AgentRuntimeService;
use crate::dto::agent_dto::{
    AgentReadTaskDetailDto, AgentTaskBriefDto, AgentTaskDetailDto, AgentTaskResultDto,
};
use crate::errors::ApplicationError;
use tt_domain::models::agent::{AgentTaskRecord, WorkspacePath};

impl AgentRuntimeService {
    pub async fn read_task_detail(
        &self,
        dto: AgentReadTaskDetailDto,
    ) -> Result<AgentTaskDetailDto, ApplicationError> {
        let run_id = dto.run_id.trim();
        let task_id = dto.task_id.trim();
        let task = self
            .invocation_repository
            .load_task(run_id, task_id)
            .await?;
        if task.run_id != run_id || task.id != task_id {
            return Err(ApplicationError::ValidationError(format!(
                "agent.task_detail_identity_mismatch: requested task `{task_id}` in run `{run_id}`, found task `{}` in run `{}`",
                task.id, task.run_id
            )));
        }
        let brief: AgentTaskBriefDto =
            serde_json::from_value(task.task.clone()).map_err(|error| {
                ApplicationError::ValidationError(format!(
                    "agent.task_brief_invalid: task `{task_id}` in run `{run_id}`: {error}"
                ))
            })?;
        if brief.objective.trim().is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.task_brief_invalid: task `{task_id}` in run `{run_id}` has an empty objective"
            )));
        }
        let result = if dto.include_result {
            self.read_task_result(run_id, &task).await?
        } else {
            None
        };
        Ok(AgentTaskDetailDto {
            run_id: task.run_id,
            task_id: task.id,
            parent_invocation_id: task.parent_invocation_id,
            child_invocation_id: task.child_invocation_id,
            target_profile_id: task.target_profile_id,
            workspace_key: task.workspace_key,
            continuation: task.continuation,
            status: task.status,
            task: brief,
            result_ref: task.result_ref,
            result,
            error: task.error,
        })
    }

    pub(super) async fn read_task_result(
        &self,
        run_id: &str,
        task: &AgentTaskRecord,
    ) -> Result<Option<AgentTaskResultDto>, ApplicationError> {
        let Some(result_ref) = &task.result_ref else {
            return Ok(None);
        };
        let invalid_result = |reason: &str| {
            ApplicationError::ValidationError(format!(
                "agent.task_result_invalid: task `{}` in run `{}`, result `{result_ref}`: {reason}",
                task.id, task.run_id
            ))
        };
        let path = WorkspacePath::parse(result_ref)?;
        let file = self.workspace_repository.read_text(run_id, &path).await?;
        let mut result: StoredTaskResult =
            serde_json::from_str(&file.text).map_err(|error| invalid_result(&error.to_string()))?;
        if result.schema_version != 1 || result.kind != "tauritavern.agentTaskResult" {
            return Err(invalid_result("unsupported result format"));
        }
        if task.run_id != run_id
            || result.runtime.task_id != task.id
            || result.runtime.run_id != run_id
            || result.runtime.child_invocation_id != task.child_invocation_id
        {
            return Err(invalid_result("result identity does not match the task"));
        }
        if result.summary.trim().is_empty() {
            return Err(invalid_result("summary must not be empty"));
        }
        // Older results used questionsForParent. Both model and UI readers use the current name.
        let legacy_questions = result.result.remove("questionsForParent");
        if result
            .result
            .get("questionsForCaller")
            .is_none_or(Value::is_null)
            && let Some(questions) = legacy_questions
        {
            result
                .result
                .insert("questionsForCaller".to_string(), questions);
        }
        Ok(Some(AgentTaskResultDto {
            summary: result.summary,
            summary_ref: result.summary_ref,
            output: result.result,
        }))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredTaskResult {
    schema_version: u32,
    kind: String,
    summary: String,
    summary_ref: Option<String>,
    result: Map<String, Value>,
    runtime: TaskResultIdentity,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskResultIdentity {
    task_id: String,
    run_id: String,
    child_invocation_id: String,
}
