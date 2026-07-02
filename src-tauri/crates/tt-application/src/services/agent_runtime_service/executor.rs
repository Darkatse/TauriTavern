use std::sync::Arc;

use serde_json::{Value, json};

use super::artifacts::build_agent_manifest;
use super::commit_ledger::RunCommitLedger;
use super::error_payload::{run_failure_payload, run_partial_success_payload};
use super::invocation::model_session_id;
use super::loop_runner::AgentLoopExit;
use super::prompt_snapshot::{prepare_agent_tool_request, request_summary};
use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::errors::ApplicationError;
use crate::services::agent_profile_service::ensure_profile_model_configured;
use tt_domain::models::agent::profile::{AgentModelBindingMode, ResolvedAgentProfile};
use tt_domain::models::agent::{
    AgentInvocationExitPolicy, AgentInvocationStatus, AgentRunEventLevel, AgentRunStatus,
    ROOT_AGENT_INVOCATION_ID, WorkspacePath,
};
use tt_domain::models::skill::SkillIndexEntry;

impl AgentRuntimeService {
    pub(super) async fn execute_agent_loop_run(
        self: Arc<Self>,
        run_id: String,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        effective_skills: Vec<SkillIndexEntry>,
        mut cancel: AgentCancelReceiver,
    ) {
        let mut commit_ledger = RunCommitLedger::default();
        let result = self
            .execute_agent_loop_run_body(
                &run_id,
                prompt_snapshot,
                request,
                resolved_profile,
                effective_skills,
                &mut commit_ledger,
                &mut cancel,
            )
            .await;

        self.finalize_agent_loop_run_result(&run_id, &commit_ledger, &result)
            .await;
        self.close_model_session_after_run(run_id);
    }

    async fn finalize_agent_loop_run_result(
        &self,
        run_id: &str,
        commit_ledger: &RunCommitLedger,
        result: &Result<(), ApplicationError>,
    ) {
        match result {
            Ok(()) => {}
            Err(ApplicationError::Cancelled(message)) => {
                let _ = self
                    .close_guidance_mailbox_for_run(
                        run_id,
                        "run_cancelled_before_next_model_request",
                        AgentRunEventLevel::Info,
                    )
                    .await;
                let _ = self.cancel_unfinished_child_tasks(run_id).await;
                let _ = self
                    .finish_root_invocation(run_id, AgentInvocationStatus::Cancelled)
                    .await;
                self.clear_pending_host_requests_for_run(run_id).await;
                let _ = self
                    .transition_status(run_id, AgentRunStatus::Cancelled)
                    .await;
                let _ = self
                    .event(
                        run_id,
                        AgentRunEventLevel::Info,
                        "run_cancelled",
                        json!({ "message": message }),
                    )
                    .await;
                self.active_runs.write().await.remove(run_id);
            }
            Err(error) => {
                let guidance_discard_reason = if commit_ledger.is_empty() {
                    "run_failed_before_next_model_request"
                } else {
                    "run_partial_success_before_next_model_request"
                };
                let _ = self
                    .close_guidance_mailbox_for_run(
                        run_id,
                        guidance_discard_reason,
                        AgentRunEventLevel::Warn,
                    )
                    .await;
                let _ = self.cancel_unfinished_child_tasks(run_id).await;
                let _ = self
                    .finish_root_invocation(run_id, AgentInvocationStatus::Failed)
                    .await;
                self.clear_pending_host_requests_for_run(run_id).await;
                if commit_ledger.is_empty() {
                    let _ = self.transition_status(run_id, AgentRunStatus::Failed).await;
                    let _ = self
                        .event(
                            run_id,
                            AgentRunEventLevel::Error,
                            "run_failed",
                            run_failure_payload(error),
                        )
                        .await;
                } else {
                    let _ = self
                        .transition_status(run_id, AgentRunStatus::PartialSuccess)
                        .await;
                    let _ = self
                        .event(
                            run_id,
                            AgentRunEventLevel::Warn,
                            "run_partial_success",
                            run_partial_success_payload(error, commit_ledger),
                        )
                        .await;
                }
                self.active_runs.write().await.remove(run_id);
            }
        }
    }

    #[cfg(feature = "test-support")]
    pub async fn execute_agent_loop_run_inner(
        &self,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let mut commit_ledger = RunCommitLedger::default();
        let run = self.run_repository.load_run(run_id).await?;
        let scope_order = super::skill_scope::skill_scope_order_for_profile(
            &resolved_profile,
            &run.skill_scope_refs,
        )?;
        let effective_skills = self
            .skill_service
            .resolve_effective_skills(&scope_order, &resolved_profile.skills)
            .await?;
        let result = self
            .execute_agent_loop_run_body(
                run_id,
                prompt_snapshot,
                request,
                resolved_profile,
                effective_skills,
                &mut commit_ledger,
                cancel,
            )
            .await;
        self.finalize_agent_loop_run_result(run_id, &commit_ledger, &result)
            .await;
        self.close_model_session_after_run(run_id.to_string());
        result
    }

    fn close_model_session_after_run(&self, run_id: String) {
        let model_gateway = Arc::clone(&self.model_gateway);
        let invocation_repository = Arc::clone(&self.invocation_repository);
        tokio::spawn(async move {
            let session_ids = match invocation_repository.list_invocations(&run_id).await {
                Ok(invocations) if !invocations.is_empty() => invocations
                    .iter()
                    .map(|invocation| model_session_id(&run_id, &invocation.id))
                    .collect::<Vec<_>>(),
                Ok(_) => vec![model_session_id(&run_id, ROOT_AGENT_INVOCATION_ID)],
                Err(error) => {
                    tracing::warn!(
                        "Failed to list agent invocations while closing model sessions for run {}: {}",
                        run_id,
                        error
                    );
                    vec![model_session_id(&run_id, ROOT_AGENT_INVOCATION_ID)]
                }
            };
            for session_id in session_ids {
                model_gateway.close_session(&session_id).await;
            }
        });
    }

    async fn execute_agent_loop_run_body(
        &self,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        effective_skills: Vec<SkillIndexEntry>,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let run = self
            .transition_status(run_id, AgentRunStatus::InitializingWorkspace)
            .await?;
        let manifest = build_agent_manifest(&run, &resolved_profile);
        self.workspace_repository
            .initialize_run(&run, &manifest, &prompt_snapshot, &resolved_profile)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "workspace_initialized",
            json!({
                "workspaceId": run.workspace_id,
                "stableChatId": run.stable_chat_id,
            }),
        )
        .await?;
        self.ensure_root_invocation(run_id, &resolved_profile)
            .await?;
        let root_invocation = self.start_root_invocation(run_id).await?;
        let invocation_id = root_invocation.id;
        let persistent_roots = manifest
            .roots
            .iter()
            .filter(|root| {
                root.commit == tt_domain::models::agent::WorkspaceRootCommit::OnRunCompleted
            })
            .map(|root| root.path.as_str())
            .collect::<Vec<_>>();
        if !persistent_roots.is_empty() {
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "persistent_projection_initialized",
                json!({
                    "roots": persistent_roots,
                }),
            )
            .await?;
        }
        self.ensure_not_cancelled(cancel)?;
        let resolved_skills = serde_json::to_string_pretty(&effective_skills).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.resolved_skills_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(
                run_id,
                &WorkspacePath::parse("input/resolved_skills.json")?,
                &resolved_skills,
            )
            .await?;

        let mut request = request;
        self.resolve_model_binding(run_id, &resolved_profile, &mut request)
            .await?;
        self.ensure_not_cancelled(cancel)?;

        let visible_tools = self.visible_tool_specs_for_invocation(
            &resolved_profile,
            AgentInvocationExitPolicy::RunFinishAllowed,
        )?;
        let request =
            prepare_agent_tool_request(request, &visible_tools, run_id, invocation_id.as_str())?;
        self.transition_status(run_id, AgentRunStatus::AssemblingContext)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "context_assembled",
            json!({
                "request": request_summary(&request),
                "invocationId": invocation_id.as_str(),
                "tools": &visible_tools,
                "maxRounds": resolved_profile.tools.max_rounds,
                "contextPolicy": &resolved_profile.context,
                "modelRetry": {
                    "maxRetries": resolved_profile.run.model_retry.max_retries,
                    "intervalMs": resolved_profile.run.model_retry.interval_ms,
                },
            }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        self.execute_active_invocation_chain(
            run_id,
            invocation_id,
            request,
            resolved_profile,
            effective_skills,
            commit_ledger,
            cancel,
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        Ok(())
    }

    async fn execute_active_invocation_chain(
        &self,
        run_id: &str,
        mut invocation_id: String,
        mut request: tt_domain::models::agent::AgentModelRequest,
        mut profile: ResolvedAgentProfile,
        mut effective_skills: Vec<SkillIndexEntry>,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let mut incoming_handoff_task_id: Option<String> = None;
        loop {
            let exit = match self
                .run_tool_loop(
                    run_id,
                    invocation_id.as_str(),
                    AgentInvocationExitPolicy::RunFinishAllowed,
                    request,
                    &profile,
                    &effective_skills,
                    commit_ledger,
                    cancel,
                )
                .await
            {
                Ok(Some(exit)) => exit,
                Ok(None) => {
                    let error = ApplicationError::ValidationError(format!(
                        "agent.max_tool_rounds_exceeded: workspace.finish or agent.handoff was not called within {} rounds",
                        profile.tools.max_rounds
                    ));
                    self.mark_active_invocation_failed(
                        run_id,
                        invocation_id.as_str(),
                        incoming_handoff_task_id.as_deref(),
                        &error,
                    )
                    .await?;
                    return Err(error);
                }
                Err(error) => {
                    self.mark_active_invocation_failed(
                        run_id,
                        invocation_id.as_str(),
                        incoming_handoff_task_id.as_deref(),
                        &error,
                    )
                    .await?;
                    return Err(error);
                }
            };

            match exit {
                AgentLoopExit::Finished => {
                    if let Err(error) = self
                        .finish_run(run_id, invocation_id.as_str(), commit_ledger, cancel)
                        .await
                    {
                        self.mark_active_invocation_failed(
                            run_id,
                            invocation_id.as_str(),
                            incoming_handoff_task_id.as_deref(),
                            &error,
                        )
                        .await?;
                        return Err(error);
                    }
                    if let Some(task_id) = incoming_handoff_task_id.as_deref() {
                        self.transition_child_task(
                            run_id,
                            task_id,
                            tt_domain::models::agent::AgentTaskStatus::Completed,
                            None,
                            None,
                        )
                        .await?;
                    }
                    return Ok(());
                }
                AgentLoopExit::Transferred {
                    task_id,
                    new_invocation_id,
                } => {
                    if let Some(incoming_task_id) = incoming_handoff_task_id.as_deref() {
                        self.transition_child_task(
                            run_id,
                            incoming_task_id,
                            tt_domain::models::agent::AgentTaskStatus::Completed,
                            None,
                            None,
                        )
                        .await?;
                    }
                    let prepared = match self
                        .prepare_handoff_invocation(
                            run_id,
                            task_id.as_str(),
                            new_invocation_id.as_str(),
                            cancel,
                        )
                        .await
                    {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            self.mark_active_invocation_failed(
                                run_id,
                                new_invocation_id.as_str(),
                                Some(task_id.as_str()),
                                &error,
                            )
                            .await?;
                            return Err(error);
                        }
                    };
                    invocation_id = new_invocation_id;
                    incoming_handoff_task_id = Some(task_id);
                    request = prepared.request;
                    profile = prepared.profile;
                    effective_skills = prepared.effective_skills;
                }
            }
        }
    }

    async fn mark_active_invocation_failed(
        &self,
        run_id: &str,
        invocation_id: &str,
        incoming_handoff_task_id: Option<&str>,
        error: &ApplicationError,
    ) -> Result<(), ApplicationError> {
        let was_cancelled = matches!(error, ApplicationError::Cancelled(_));
        let task_status = if was_cancelled {
            tt_domain::models::agent::AgentTaskStatus::Cancelled
        } else {
            tt_domain::models::agent::AgentTaskStatus::Failed
        };
        let invocation_status = if was_cancelled {
            AgentInvocationStatus::Cancelled
        } else {
            AgentInvocationStatus::Failed
        };
        if let Some(task_id) = incoming_handoff_task_id {
            self.transition_child_task(run_id, task_id, task_status, None, Some(error.to_string()))
                .await?;
        }
        if invocation_id != ROOT_AGENT_INVOCATION_ID {
            self.finish_invocation(run_id, invocation_id, invocation_status)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn resolve_model_binding(
        &self,
        run_id: &str,
        profile: &ResolvedAgentProfile,
        request: &mut ChatCompletionGenerateRequestDto,
    ) -> Result<(), ApplicationError> {
        match profile.model.mode {
            AgentModelBindingMode::CurrentPromptSnapshot => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Info,
                    "agent_model_binding_resolved",
                    json!({
                        "mode": "currentPromptSnapshot",
                        "chatCompletionSource": request
                            .payload
                            .get("chat_completion_source")
                            .and_then(Value::as_str),
                        "customApiFormat": request
                            .payload
                            .get("custom_api_format")
                            .and_then(Value::as_str),
                        "modelId": request.payload.get("model").and_then(Value::as_str),
                    }),
                )
                .await?;
            }
            AgentModelBindingMode::RequiresConfiguration => {
                ensure_profile_model_configured(profile)?;
            }
            AgentModelBindingMode::ConnectionRef => {
                let connection_ref = profile.model.connection_ref.as_deref().ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "agent.model_connection_ref_required: model.connectionRef is required when model.mode is connectionRef"
                            .to_string(),
                    )
                })?;
                let model_id = profile.model.model_id.as_deref().ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "agent.model_id_required: model.modelId is required when model.mode is connectionRef"
                            .to_string(),
                    )
                })?;
                let resolved = self
                    .llm_connection_service
                    .apply_connection_to_payload(connection_ref, model_id, &mut request.payload)
                    .await?;
                let payload = serde_json::to_value(resolved).map_err(|error| {
                    ApplicationError::ValidationError(format!(
                        "agent.model_binding_resolved_serialize_failed: {error}"
                    ))
                })?;
                self.event(
                    run_id,
                    AgentRunEventLevel::Info,
                    "agent_model_binding_resolved",
                    payload,
                )
                .await?;
            }
        }

        Ok(())
    }
}
