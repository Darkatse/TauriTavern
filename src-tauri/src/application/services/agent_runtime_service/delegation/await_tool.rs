use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::rendering::render_await_content;
use super::task_status::task_is_terminal;
use super::tool_error::tool_error_outcome;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_runtime_service::AgentCancelReceiver;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::domain::models::agent::{
    AgentRunEventLevel, AgentTaskRecord, AgentTaskStatus, AgentToolCall, AgentToolResult,
    WorkspacePath,
};

const DEFAULT_AGENT_AWAIT_TIMEOUT_MS: u64 = 120_000;
const MAX_AGENT_AWAIT_TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentAwaitArgs {
    #[serde(default)]
    task_ids: Option<Vec<String>>,
    #[serde(default)]
    mode: Option<AgentAwaitMode>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum AgentAwaitMode {
    NextCompleted,
    AllCompleted,
    StatusOnly,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::application::services::agent_runtime_service) struct AwaitTaskView {
    task_id: String,
    agent_id: String,
    status: AgentTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confidence: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifacts: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    findings: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    warnings: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suggested_next_actions: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    questions_for_caller: Option<Value>,
}

impl AgentRuntimeService {
    pub(in crate::application::services::agent_runtime_service) async fn dispatch_agent_await_tool(
        &self,
        run_id: &str,
        invocation_id: &str,
        call: &AgentToolCall,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let args = match serde_json::from_value::<AgentAwaitArgs>(call.arguments.clone()) {
            Ok(args) => args,
            Err(error) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &format!("invalid agent.await arguments: {error}"),
                    started.elapsed().as_millis(),
                ));
            }
        };
        let mode = args.mode.unwrap_or(AgentAwaitMode::NextCompleted);
        let timeout_ms = args.timeout_ms.unwrap_or(DEFAULT_AGENT_AWAIT_TIMEOUT_MS);
        if timeout_ms > MAX_AGENT_AWAIT_TIMEOUT_MS {
            return Ok(tool_error_outcome(
                call,
                "tool.invalid_arguments",
                &format!("timeoutMs must be <= {MAX_AGENT_AWAIT_TIMEOUT_MS}"),
                started.elapsed().as_millis(),
            ));
        }
        let selected_ids = match normalize_task_ids(args.task_ids) {
            Ok(ids) => ids,
            Err(message) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
        };
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_await_started",
            json!({
                "parentInvocationId": invocation_id,
                "mode": mode,
                "taskIds": selected_ids,
                "timeoutMs": timeout_ms,
            }),
        )
        .await?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let scheduler = self.active_run_handle(run_id).await?.scheduler.clone();
        let mut task_events = scheduler.subscribe();
        let (tasks, timed_out) = loop {
            self.ensure_not_cancelled(cancel)?;
            let tasks = match self
                .selected_child_tasks(run_id, invocation_id, selected_ids.as_ref())
                .await
            {
                Ok(tasks) => tasks,
                Err(ApplicationError::ValidationError(message))
                    if message.contains("agent.await_task_not_found") =>
                {
                    return Ok(tool_error_outcome(
                        call,
                        "agent.await_task_not_found",
                        &message,
                        started.elapsed().as_millis(),
                    ));
                }
                Err(error) => return Err(error),
            };
            if tasks.is_empty() {
                return Ok(tool_error_outcome(
                    call,
                    "agent.no_child_tasks",
                    "No delegated tasks are selected.",
                    started.elapsed().as_millis(),
                ));
            }
            if await_condition_met(&tasks, mode) {
                break (tasks, false);
            }
            if mode == AgentAwaitMode::StatusOnly || Instant::now() >= deadline {
                break (tasks, mode != AgentAwaitMode::StatusOnly);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                changed = task_events.changed() => {
                    changed.map_err(|_| ApplicationError::InternalError(format!(
                        "agent.task_scheduler_closed: active run `{run_id}` task scheduler closed while awaiting delegated tasks"
                    )))?;
                }
                _ = tokio::time::sleep(remaining) => {}
                changed = cancel.changed() => {
                    let _ = changed;
                    self.ensure_not_cancelled(cancel)?;
                }
            }
        };

        let views = self.await_task_views(run_id, &tasks).await?;
        let structured = json!({
            "mode": mode,
            "timedOut": timed_out,
            "tasks": views,
        });
        let content = render_await_content(&structured);
        self.event(
            run_id,
            if timed_out {
                AgentRunEventLevel::Warn
            } else {
                AgentRunEventLevel::Info
            },
            "agent_await_completed",
            json!({
                "parentInvocationId": invocation_id,
                "mode": mode,
                "timedOut": timed_out,
                "taskCount": structured["tasks"].as_array().map(Vec::len).unwrap_or(0),
            }),
        )
        .await?;

        Ok(AgentToolDispatchOutcome {
            result: AgentToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content,
                structured,
                is_error: false,
                error_code: None,
                resource_refs: Vec::new(),
            },
            effect: AgentToolEffect::None,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    async fn selected_child_tasks(
        &self,
        run_id: &str,
        invocation_id: &str,
        selected_ids: Option<&Vec<String>>,
    ) -> Result<Vec<AgentTaskRecord>, ApplicationError> {
        let tasks = self.invocation_repository.list_tasks(run_id).await?;
        let mut owned = tasks
            .into_iter()
            .filter(|task| task.parent_invocation_id == invocation_id)
            .collect::<Vec<_>>();
        let Some(selected_ids) = selected_ids else {
            return Ok(owned);
        };
        let mut selected = Vec::with_capacity(selected_ids.len());
        for selected_id in selected_ids {
            let Some(index) = owned.iter().position(|task| task.id == *selected_id) else {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.await_task_not_found: task `{selected_id}` does not belong to invocation `{invocation_id}`"
                )));
            };
            selected.push(owned.swap_remove(index));
        }
        selected.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(selected)
    }

    pub(in crate::application::services::agent_runtime_service) async fn await_task_views(
        &self,
        run_id: &str,
        tasks: &[AgentTaskRecord],
    ) -> Result<Vec<AwaitTaskView>, ApplicationError> {
        let mut views = Vec::with_capacity(tasks.len());
        for task in tasks {
            let result = match task.result_ref.as_ref() {
                Some(path) => {
                    let path = WorkspacePath::parse(path)?;
                    let file = self.workspace_repository.read_text(run_id, &path).await?;
                    Some(serde_json::from_str::<Value>(&file.text).map_err(|error| {
                        ApplicationError::ValidationError(format!(
                            "agent.await_result_invalid: result `{}` is invalid JSON: {error}",
                            path.as_str()
                        ))
                    })?)
                }
                None => None,
            };
            views.push(AwaitTaskView {
                task_id: task.id.clone(),
                agent_id: task.target_profile_id.clone(),
                status: task.status,
                error: task.error.clone(),
                summary: result_summary(result.as_ref()),
                confidence: result_payload_field(result.as_ref(), "confidence"),
                artifacts: result_payload_field(result.as_ref(), "artifacts"),
                findings: result_payload_field(result.as_ref(), "findings"),
                warnings: result_payload_field(result.as_ref(), "warnings"),
                suggested_next_actions: result_payload_field(
                    result.as_ref(),
                    "suggestedNextActions",
                ),
                questions_for_caller: result_payload_field(result.as_ref(), "questionsForCaller")
                    .or_else(|| result_payload_field(result.as_ref(), "questionsForParent")),
            });
        }
        Ok(views)
    }

    pub(in crate::application::services::agent_runtime_service) async fn completed_child_results_message(
        &self,
        run_id: &str,
        invocation_id: &str,
        seen_task_ids: &mut HashSet<String>,
    ) -> Result<Option<String>, ApplicationError> {
        let tasks = self
            .invocation_repository
            .list_tasks(run_id)
            .await?
            .into_iter()
            .filter(|task| task.parent_invocation_id == invocation_id)
            .filter(|task| task_is_terminal(task.status))
            .filter(|task| !seen_task_ids.contains(&task.id))
            .collect::<Vec<_>>();
        if tasks.is_empty() {
            return Ok(None);
        }
        let views = self.await_task_views(run_id, &tasks).await?;
        for task in &tasks {
            seen_task_ids.insert(task.id.clone());
        }
        let structured = json!({
            "mode": "backgroundResults",
            "timedOut": false,
            "tasks": views,
        });
        Ok(Some(format!(
            "Delegated task results are now available. Review them before deciding your next action.\n\n{}",
            render_await_content(&structured)
        )))
    }
}

fn normalize_task_ids(task_ids: Option<Vec<String>>) -> Result<Option<Vec<String>>, String> {
    let Some(task_ids) = task_ids else {
        return Ok(None);
    };
    let mut normalized = Vec::with_capacity(task_ids.len());
    for task_id in task_ids {
        let task_id = task_id.trim();
        if task_id.is_empty() {
            return Err("taskIds cannot contain empty values".to_string());
        }
        if normalized.iter().any(|existing| existing == task_id) {
            return Err(format!("duplicate task id `{task_id}`"));
        }
        normalized.push(task_id.to_string());
    }
    Ok(Some(normalized))
}

fn await_condition_met(tasks: &[AgentTaskRecord], mode: AgentAwaitMode) -> bool {
    match mode {
        AgentAwaitMode::StatusOnly => true,
        AgentAwaitMode::NextCompleted => tasks.iter().any(|task| task_is_terminal(task.status)),
        AgentAwaitMode::AllCompleted => tasks.iter().all(|task| task_is_terminal(task.status)),
    }
}

fn result_summary(result_doc: Option<&Value>) -> Option<String> {
    result_doc
        .and_then(|result| result.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(str::to_string)
}

fn result_payload_field(result_doc: Option<&Value>, key: &str) -> Option<Value> {
    result_doc
        .and_then(|result| result.get("result"))
        .and_then(|result| result.get(key))
        .filter(|value| !value.is_null())
        .cloned()
}
