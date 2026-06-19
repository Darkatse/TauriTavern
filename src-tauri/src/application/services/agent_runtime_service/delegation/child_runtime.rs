use serde_json::{Map, Value, json};

use super::super::loop_runner::AgentLoopExit;
use super::policy::apply_child_invocation_policy;
use super::rendering::{render_child_task_prompt, render_handoff_task_prompt};
use super::task_status::task_is_terminal;
use crate::application::dto::agent_dto::AgentPromptAssemblyScopeDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, ensure_profile_model_configured, materialize_agent_system_prompt,
};
use crate::application::services::agent_runtime_service::AgentCancelReceiver;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_runtime_service::commit_ledger::RunCommitLedger;
use crate::application::services::agent_runtime_service::prompt_snapshot::{
    prepare_agent_tool_request, request_from_prompt_snapshot, request_summary,
};
use crate::application::services::agent_runtime_service::skill_scope::{
    skill_event_summary, skill_scope_order_for_profile,
};
use crate::domain::models::agent::profile::{AgentPresetBindingMode, ResolvedAgentProfile};
use crate::domain::models::agent::{
    AgentDelegationContinuation, AgentInvocationExitPolicy, AgentInvocationStatus,
    AgentModelRequest, AgentRunEventLevel, AgentRunSkillScopeRefs, AgentTaskRecord,
    AgentTaskStatus, WorkspacePath,
};
use crate::domain::models::skill::{SkillIndexEntry, SkillScope};

pub(in crate::application::services::agent_runtime_service) struct PreparedDelegatedInvocationContext
{
    pub(in crate::application::services::agent_runtime_service) profile: ResolvedAgentProfile,
    pub(in crate::application::services::agent_runtime_service) request: AgentModelRequest,
    pub(in crate::application::services::agent_runtime_service) effective_skills:
        Vec<SkillIndexEntry>,
}

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
                    && task.continuation == AgentDelegationContinuation::ReturnToParent
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

        let mut profile = self.resolve_task_profile(&task).await?;
        ensure_profile_model_configured(&profile)?;
        apply_child_invocation_policy(&mut profile, task.budget)?;
        let PreparedDelegatedInvocationContext {
            profile,
            request,
            effective_skills,
        } = self
            .prepare_delegated_invocation_context(
                run_id,
                invocation_id,
                &task,
                profile,
                AgentInvocationExitPolicy::TaskReturnRequired,
                "subagent",
                "taskReturnRequired",
                render_child_task_prompt(&task),
                cancel,
            )
            .await?;
        let mut child_commit_ledger = RunCommitLedger::default();
        let exit = self
            .run_tool_loop(
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
        if let AgentLoopExit::Transferred { .. } = exit {
            return Err(ApplicationError::ValidationError(
                "agent.child_handoff_denied: delegated tasks cannot hand off".to_string(),
            ));
        }

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

    pub(in crate::application::services::agent_runtime_service) async fn resolve_task_profile(
        &self,
        task: &AgentTaskRecord,
    ) -> Result<ResolvedAgentProfile, ApplicationError> {
        self.profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id: Some(task.target_profile_id.as_str()),
                known_tools: self.tool_registry.specs(),
            })
            .await
    }

    pub(in crate::application::services::agent_runtime_service) async fn prepare_handoff_invocation(
        &self,
        run_id: &str,
        task_id: &str,
        invocation_id: &str,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<PreparedDelegatedInvocationContext, ApplicationError> {
        let task = self
            .transition_child_task(run_id, task_id, AgentTaskStatus::Running, None, None)
            .await?;
        if task.status != AgentTaskStatus::Running {
            return Err(ApplicationError::Cancelled(format!(
                "Handoff task `{task_id}` was cancelled before it started"
            )));
        }
        self.start_invocation(run_id, invocation_id).await?;
        let profile = self.resolve_task_profile(&task).await?;
        ensure_profile_model_configured(&profile)?;
        self.prepare_delegated_invocation_context(
            run_id,
            invocation_id,
            &task,
            profile,
            AgentInvocationExitPolicy::RunFinishAllowed,
            "handoff",
            "runFinishAllowed",
            render_handoff_task_prompt(&task),
            cancel,
        )
        .await
    }

    pub(in crate::application::services::agent_runtime_service) async fn prepare_delegated_invocation_context(
        &self,
        run_id: &str,
        invocation_id: &str,
        task: &AgentTaskRecord,
        profile: ResolvedAgentProfile,
        exit_policy: AgentInvocationExitPolicy,
        invocation_kind: &str,
        exit_policy_label: &str,
        task_prompt: String,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<PreparedDelegatedInvocationContext, ApplicationError> {
        let prompt_snapshot = self
            .workspace_repository
            .read_text(run_id, &WorkspacePath::parse("input/prompt_snapshot.json")?)
            .await?;
        let prompt_snapshot: Value = serde_json::from_str(&prompt_snapshot.text).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.invalid_prompt_snapshot: input/prompt_snapshot.json is invalid JSON: {error}"
            ))
        })?;
        let visible_tools = self.visible_tool_specs_for_invocation(&profile, exit_policy)?;
        let run = self.run_repository.load_run(run_id).await?;
        let invocation_prompt_snapshot = if profile.preset.mode == AgentPresetBindingMode::Ref {
            self.assemble_invocation_prompt_snapshot(
                run_id,
                invocation_id,
                &profile,
                &visible_tools,
                run.generation_type.as_str(),
                frozen_run_input_snapshot_from_prompt_snapshot(&prompt_snapshot)?,
                AgentPromptAssemblyScopeDto {
                    run_id: run_id.to_string(),
                    invocation_id: invocation_id.to_string(),
                    invocation_kind: invocation_kind.to_string(),
                    parent_invocation_id: Some(task.parent_invocation_id.clone()),
                    task_id: Some(task.id.clone()),
                    exit_policy: Some(exit_policy_label.to_string()),
                },
                task_prompt.clone(),
                cancel,
            )
            .await?
        } else {
            None
        };
        let mut request = if let Some(invocation_prompt_snapshot) = invocation_prompt_snapshot {
            request_from_prompt_snapshot(&invocation_prompt_snapshot)?
        } else {
            let mut request = request_from_prompt_snapshot(&prompt_snapshot)?;
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
                        "content": task_prompt
                    }
                ]),
            );
            request
        };
        self.resolve_model_binding(run_id, &profile, &mut request)
            .await?;
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

        let (skill_scope_order, effective_skills) = self
            .resolve_child_effective_skills(&profile, &run.skill_scope_refs)
            .await?;
        let resolved_skills = serde_json::to_string_pretty(&effective_skills).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.resolved_skills_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(
                run_id,
                &WorkspacePath::parse(format!(
                    "input/invocations/{invocation_id}/resolved_skills.json"
                ))?,
                &resolved_skills,
            )
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "skill_scopes_resolved",
            json!({
                "invocationId": invocation_id,
                "profileId": profile.id.as_str(),
                "scopes": skill_scope_order,
                "refs": &run.skill_scope_refs,
                "effectiveSkills": skill_event_summary(&effective_skills),
            }),
        )
        .await?;

        Ok(PreparedDelegatedInvocationContext {
            profile,
            request,
            effective_skills,
        })
    }

    async fn resolve_child_effective_skills(
        &self,
        profile: &ResolvedAgentProfile,
        refs: &AgentRunSkillScopeRefs,
    ) -> Result<(Vec<SkillScope>, Vec<SkillIndexEntry>), ApplicationError> {
        let scope_order = skill_scope_order_for_profile(profile, refs)?;
        let effective_skills = self
            .skill_service
            .resolve_effective_skills(&scope_order, &profile.skills)
            .await?;
        Ok((scope_order, effective_skills))
    }
}

fn frozen_run_input_snapshot_from_prompt_snapshot(
    prompt_snapshot: &Value,
) -> Result<Value, ApplicationError> {
    let object = prompt_snapshot_object(prompt_snapshot)?;
    object
        .get("frozenRunInputSnapshot")
        .or_else(|| object.get("frozen_run_input_snapshot"))
        .cloned()
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.child_prompt_assembly_frozen_snapshot_required: child preset prompt assembly requires frozenRunInputSnapshot in input/prompt_snapshot.json"
                    .to_string(),
            )
        })
}

fn prompt_snapshot_object(
    prompt_snapshot: &Value,
) -> Result<&Map<String, Value>, ApplicationError> {
    prompt_snapshot.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: prompt snapshot must be an object".to_string(),
        )
    })
}
