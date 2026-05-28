use serde_json::{Value, json};

use super::policy::apply_child_invocation_policy;
use super::rendering::render_child_task_prompt;
use super::task_status::task_is_terminal;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, materialize_agent_system_prompt,
};
use crate::application::services::agent_runtime_service::AgentCancelReceiver;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_runtime_service::commit_ledger::RunCommitLedger;
use crate::application::services::agent_runtime_service::prompt_snapshot::{
    prepare_agent_tool_request, request_from_prompt_snapshot, request_summary,
};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentInvocationExitPolicy, AgentInvocationStatus, AgentRunEventLevel, AgentTaskStatus,
    WorkspacePath,
};
use crate::domain::models::skill::{SkillIndexEntry, SkillScope};

impl AgentRuntimeService {
    pub(in crate::application::services::agent_runtime_service) async fn has_pending_child_tasks(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<bool, ApplicationError> {
        Ok(self
            .invocation_repository
            .list_tasks(run_id)
            .await?
            .into_iter()
            .any(|task| {
                task.parent_invocation_id == invocation_id
                    && matches!(
                        task.status,
                        AgentTaskStatus::Queued | AgentTaskStatus::Running
                    )
            }))
    }

    pub(in crate::application::services::agent_runtime_service) async fn run_child_task_to_terminal(
        &self,
        run_id: &str,
        task_id: &str,
        invocation_id: &str,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let result =
            Box::pin(self.execute_child_invocation_body(run_id, task_id, invocation_id, cancel))
                .await;
        if let Err(error) = result {
            let was_cancelled = matches!(error, ApplicationError::Cancelled(_));
            let task_status = if was_cancelled {
                AgentTaskStatus::Cancelled
            } else {
                AgentTaskStatus::Failed
            };
            let invocation_status = if task_status == AgentTaskStatus::Cancelled {
                AgentInvocationStatus::Cancelled
            } else {
                AgentInvocationStatus::Failed
            };
            let message = error.to_string();
            let transition = self
                .transition_child_task_with_change(
                    run_id,
                    task_id,
                    task_status,
                    None,
                    Some(message.clone()),
                )
                .await?;
            if !transition.changed {
                return Ok(());
            }
            self.finish_child_invocation(run_id, invocation_id, invocation_status)
                .await?;
            if was_cancelled {
                return Ok(());
            }
            self.event(
                run_id,
                AgentRunEventLevel::Warn,
                "agent_child_invocation_failed",
                json!({
                    "taskId": task_id,
                    "childInvocationId": invocation_id,
                    "message": message,
                }),
            )
            .await?;
        }
        Ok(())
    }

    async fn execute_child_invocation_body(
        &self,
        run_id: &str,
        task_id: &str,
        invocation_id: &str,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        self.ensure_not_cancelled(cancel)?;
        let task = self
            .transition_child_task(run_id, task_id, AgentTaskStatus::Running, None, None)
            .await?;
        if task.status != AgentTaskStatus::Running {
            return Err(ApplicationError::Cancelled(format!(
                "Delegated task `{task_id}` was cancelled before it started"
            )));
        }
        self.start_child_invocation(run_id, invocation_id).await?;

        let mut profile = self
            .profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id: Some(task.target_profile_id.as_str()),
                known_tools: self.tool_registry.specs(),
            })
            .await?;
        apply_child_invocation_policy(&mut profile, task.budget)?;

        let prompt_snapshot = self
            .workspace_repository
            .read_text(run_id, &WorkspacePath::parse("input/prompt_snapshot.json")?)
            .await?;
        let prompt_snapshot: Value = serde_json::from_str(&prompt_snapshot.text).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.invalid_prompt_snapshot: input/prompt_snapshot.json is invalid JSON: {error}"
            ))
        })?;
        let mut request = request_from_prompt_snapshot(&prompt_snapshot)?;
        self.resolve_model_binding(run_id, &profile, &mut request)
            .await?;

        let visible_tools = self.visible_tool_specs_for_invocation(
            &profile,
            AgentInvocationExitPolicy::TaskReturnRequired,
        )?;
        let system_prompt = materialize_agent_system_prompt(&visible_tools, &profile);
        request.payload.insert(
            "messages".to_string(),
            json!([
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": render_child_task_prompt(&task)
                }
            ]),
        );
        let request = prepare_agent_tool_request(request, &visible_tools, run_id, invocation_id)?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "context_assembled",
            json!({
                "request": request_summary(&request),
                "invocationId": invocation_id,
                "tools": &visible_tools,
                "maxRounds": profile.tools.max_rounds,
                "contextPolicy": &profile.context,
            }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        let effective_skills = self.resolve_child_effective_skills(&profile).await?;
        let mut child_commit_ledger = RunCommitLedger::default();
        self.run_tool_loop(
            run_id,
            invocation_id,
            AgentInvocationExitPolicy::TaskReturnRequired,
            request,
            &profile,
            &effective_skills,
            &mut child_commit_ledger,
            cancel,
        )
        .await?
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.max_tool_rounds_exceeded: task.return was not called within {} rounds",
                profile.tools.max_rounds
            ))
        })?;

        let task = self
            .invocation_repository
            .load_task(run_id, task_id)
            .await?;
        if !task_is_terminal(task.status) {
            return Err(ApplicationError::ValidationError(format!(
                "agent.child_invocation_missing_return: child invocation `{invocation_id}` ended without terminal task status"
            )));
        }
        self.finish_child_invocation(run_id, invocation_id, AgentInvocationStatus::Completed)
            .await?;
        Ok(())
    }

    async fn resolve_child_effective_skills(
        &self,
        profile: &ResolvedAgentProfile,
    ) -> Result<Vec<SkillIndexEntry>, ApplicationError> {
        self.skill_service
            .resolve_effective_skills(
                &[
                    SkillScope::Global,
                    SkillScope::Profile {
                        profile_id: profile.id.as_str().to_string(),
                    },
                ],
                &profile.skills,
            )
            .await
    }
}
