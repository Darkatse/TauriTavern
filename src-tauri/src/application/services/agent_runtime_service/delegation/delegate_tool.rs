use std::collections::HashSet;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::policy::{AgentDelegateBudget, validate_delegate_budget, validate_subagent_target};
use super::tool_error::tool_error_outcome;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::AgentProfileResolveInput;
use crate::application::services::agent_runtime_service::AgentCancelReceiver;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::domain::models::agent::profile::{AgentProfileId, ResolvedAgentProfile};
use crate::domain::models::agent::{
    AgentRunEventLevel, AgentTaskBudget, AgentTaskStatus, AgentToolCall, AgentToolResult,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentDelegateArgs {
    agent_id: String,
    task: Value,
    #[serde(default)]
    budget: Option<AgentDelegateBudget>,
}

impl AgentRuntimeService {
    pub(in crate::application::services::agent_runtime_service) async fn dispatch_agent_delegate_tool(
        &self,
        run_id: &str,
        invocation_id: &str,
        call: &AgentToolCall,
        profile: &ResolvedAgentProfile,
        _cancel: &AgentCancelReceiver,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let args = match serde_json::from_value::<AgentDelegateArgs>(call.arguments.clone()) {
            Ok(args) => args,
            Err(error) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &format!("invalid agent.delegate arguments: {error}"),
                    started.elapsed().as_millis(),
                ));
            }
        };
        if let Err(message) = validate_delegate_task_packet(&args.task) {
            return Ok(tool_error_outcome(
                call,
                "tool.invalid_arguments",
                &message,
                started.elapsed().as_millis(),
            ));
        }
        if !profile.delegation.can_delegate {
            return Ok(tool_error_outcome(
                call,
                "agent.delegation_policy_denied",
                &format!(
                    "agent.profile_cannot_delegate: profile `{}` cannot delegate to subagents",
                    profile.id.as_str()
                ),
                started.elapsed().as_millis(),
            ));
        }
        let target_id = match AgentProfileId::parse(&args.agent_id) {
            Ok(target_id) => target_id,
            Err(message) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
        };
        let target = match self
            .profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id: Some(target_id.as_str()),
                known_tools: self.tool_registry.specs(),
            })
            .await
        {
            Ok(target) => target,
            Err(ApplicationError::NotFound(message)) => {
                return Ok(tool_error_outcome(
                    call,
                    "agent.target_profile_not_found",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
            Err(error) => return Err(error),
        };
        if let Err(message) = validate_subagent_target(profile, &target) {
            return Ok(tool_error_outcome(
                call,
                "agent.delegation_policy_denied",
                &message,
                started.elapsed().as_millis(),
            ));
        }
        if let Err(message) = validate_delegate_budget(args.budget, &target) {
            return Ok(tool_error_outcome(
                call,
                "tool.invalid_arguments",
                &message,
                started.elapsed().as_millis(),
            ));
        }
        if let Err(message) = self
            .validate_parent_delegate_budget(run_id, invocation_id, profile)
            .await?
        {
            return Ok(tool_error_outcome(
                call,
                "agent.delegation_budget_exhausted",
                &message,
                started.elapsed().as_millis(),
            ));
        }

        let task_id = format!("task_{}", Uuid::new_v4().simple());
        let child_invocation_id = format!("inv_{}", Uuid::new_v4().simple());
        let workspace_key = self
            .allocate_child_workspace_key(run_id, target.id.as_str())
            .await?;
        let task = self
            .create_child_task(
                run_id,
                invocation_id,
                child_invocation_id.clone(),
                task_id.clone(),
                target.id.as_str().to_string(),
                workspace_key,
                call.id.clone(),
                args.task.clone(),
                args.budget.map(AgentTaskBudget::from),
            )
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_delegate_started",
            json!({
                "taskId": task.id.as_str(),
                "parentInvocationId": invocation_id,
                "childInvocationId": task.child_invocation_id.as_str(),
                "targetProfileId": task.target_profile_id.as_str(),
                "workspaceKey": task.workspace_key.as_str(),
            }),
        )
        .await?;
        self.active_run_handle(run_id)
            .await?
            .scheduler
            .submit(task.id.clone(), task.child_invocation_id.clone())?;

        let structured = json!({
            "taskId": task_id,
            "status": task.status,
            "agentId": target.id.as_str(),
        });
        Ok(AgentToolDispatchOutcome {
            result: AgentToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: format!(
                    "Started delegated task {} with Agent {}. You can continue other work; use agent_await only when your next decision needs this task's result or current status.",
                    structured["taskId"].as_str().unwrap_or(""),
                    target.id.as_str()
                ),
                structured,
                is_error: false,
                error_code: None,
                resource_refs: Vec::new(),
            },
            effect: AgentToolEffect::None,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    async fn validate_parent_delegate_budget(
        &self,
        run_id: &str,
        invocation_id: &str,
        profile: &ResolvedAgentProfile,
    ) -> Result<Result<(), String>, ApplicationError> {
        let tasks = self.invocation_repository.list_tasks(run_id).await?;
        let owned = tasks
            .iter()
            .filter(|task| task.parent_invocation_id == invocation_id)
            .collect::<Vec<_>>();
        if owned.len() >= profile.delegation.max_invocations_per_run {
            return Ok(Err(format!(
                "agent.max_invocations_per_run_exhausted: profile `{}` may create at most {} subagent tasks per run",
                profile.id.as_str(),
                profile.delegation.max_invocations_per_run
            )));
        }
        let pending = owned
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    AgentTaskStatus::Queued | AgentTaskStatus::Running
                )
            })
            .count();
        if pending >= profile.delegation.max_concurrent_invocations {
            return Ok(Err(format!(
                "agent.max_concurrent_invocations_exhausted: profile `{}` may run at most {} concurrent subagent tasks",
                profile.id.as_str(),
                profile.delegation.max_concurrent_invocations
            )));
        }
        Ok(Ok(()))
    }

    async fn allocate_child_workspace_key(
        &self,
        run_id: &str,
        target_profile_id: &str,
    ) -> Result<String, ApplicationError> {
        let tasks = self.invocation_repository.list_tasks(run_id).await?;
        Ok(next_child_workspace_key(
            target_profile_id,
            tasks.iter().map(|task| task.workspace_key.as_str()),
        ))
    }
}

fn validate_delegate_task_packet(task: &Value) -> Result<(), String> {
    let object = task
        .as_object()
        .ok_or_else(|| "task must be an object".to_string())?;
    validate_required_task_string(object, "objective")?;
    if let Some(value) = object.get("title") {
        validate_optional_task_string(value, "title")?;
    }
    Ok(())
}

fn validate_required_task_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<(), String> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("task.{key} must be a non-empty string"))?;
    validate_task_string_len(value, key)
}

fn validate_optional_task_string(value: &Value, key: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let value = value
        .as_str()
        .ok_or_else(|| format!("task.{key} must be a string when provided"))?
        .trim();
    if value.is_empty() {
        return Ok(());
    }
    validate_task_string_len(value, key)
}

fn validate_task_string_len(value: &str, key: &str) -> Result<(), String> {
    if value.len() > 4_000 {
        return Err(format!("task.{key} must be <= 4000 chars"));
    }
    Ok(())
}

fn next_child_workspace_key<'a>(
    target_profile_id: &str,
    existing_keys: impl IntoIterator<Item = &'a str>,
) -> String {
    let existing_keys = existing_keys.into_iter().collect::<HashSet<_>>();
    if !existing_keys.contains(target_profile_id) {
        return target_profile_id.to_string();
    }

    for index in 2.. {
        let candidate = format!("{target_profile_id}-{index:03}");
        if !existing_keys.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("unbounded workspace key allocation exhausted")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{next_child_workspace_key, validate_delegate_task_packet};

    #[test]
    fn child_workspace_key_uses_agent_id_then_numbered_suffixes() {
        assert_eq!(next_child_workspace_key("scene-critic", []), "scene-critic");
        assert_eq!(
            next_child_workspace_key("scene-critic", ["scene-critic"]),
            "scene-critic-002"
        );
        assert_eq!(
            next_child_workspace_key("scene-critic", ["scene-critic", "scene-critic-002"]),
            "scene-critic-003"
        );
    }

    #[test]
    fn delegate_task_packet_accepts_missing_title() {
        let task = json!({
            "objective": "Find one concrete improvement.",
            "context": { "draft": "A quiet scene." }
        });

        assert!(validate_delegate_task_packet(&task).is_ok());
    }

    #[test]
    fn delegate_task_packet_requires_objective() {
        let error = validate_delegate_task_packet(&json!({ "title": "Critique" }))
            .expect_err("missing objective should fail");

        assert_eq!(error, "task.objective must be a non-empty string");
    }

    #[test]
    fn delegate_task_packet_rejects_non_string_title_when_provided() {
        let error = validate_delegate_task_packet(&json!({
            "title": { "text": "Critique" },
            "objective": "Find one concrete improvement."
        }))
        .expect_err("non-string title should fail");

        assert_eq!(error, "task.title must be a string when provided");
    }
}
