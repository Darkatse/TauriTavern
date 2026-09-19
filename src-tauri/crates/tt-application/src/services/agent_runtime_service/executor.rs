use std::sync::Arc;

use serde_json::{Value, json};
#[cfg(feature = "test-support")]
use tokio::sync::watch;

use super::artifacts::build_agent_manifest;
use super::continuation::{InvocationFrame, InvocationStep, RunExecutionState};
use super::error_payload::{run_failure_payload, run_partial_success_payload};
use super::invocation::model_session_id;
use super::loop_runner::AgentLoopExit;
use super::prompt_snapshot::{
    frozen_macros_from_snapshot, prepare_agent_tool_request, request_summary,
};
use super::tool_snapshot::tool_snapshot_summary;
use super::{AgentCancelReceiver, AgentRuntimeService, PreparedInvocation};
use crate::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::errors::ApplicationError;
use crate::services::agent_profile_service::ensure_profile_model_configured;
use tt_domain::models::agent::profile::{AgentModelBindingMode, ResolvedAgentProfile};
use tt_domain::models::agent::{
    AgentInvocationExitPolicy, AgentInvocationStatus, AgentRunEventLevel, AgentRunStatus,
    WorkspacePath,
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
        let mut state = RunExecutionState::default();
        let result = self
            .execute_agent_loop_run_body(
                &run_id,
                prompt_snapshot,
                request,
                resolved_profile,
                effective_skills,
                &mut state,
                &mut cancel,
            )
            .await;
        self.finish_execution(&run_id, state, result).await;
    }

    pub(super) async fn execute_resumed_run(
        self: Arc<Self>,
        run_id: String,
        mut state: RunExecutionState,
        mut cancel: AgentCancelReceiver,
    ) {
        let result = async {
            let handle = self.active_run_handle(&run_id).await?;
            for frame in state.foreground.iter_mut().chain(&mut state.children) {
                if !matches!(
                    frame.progress.step,
                    InvocationStep::Exited(AgentLoopExit::Transferred { .. })
                ) {
                    self.reopen_invocation(frame).await?;
                }
            }
            for child in state.children.drain(..) {
                handle.scheduler.submit_resumed(child)?;
            }
            self.execute_active_invocation_chain(&mut state, &mut cancel)
                .await
        }
        .await;
        self.finish_execution(&run_id, state, result).await;
    }

    async fn finish_execution(
        &self,
        run_id: &str,
        state: RunExecutionState,
        result: Result<(), ApplicationError>,
    ) {
        if let Err(error) = self
            .finalize_agent_loop_run_result(run_id, state, result.as_ref().err(), None)
            .await
        {
            tracing::error!(target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Agent run {run_id} could not save its final execution state: {error}");
            let handle = self.active_runs.read().await.get(run_id).cloned();
            let has_pending = if let Some(handle) = handle {
                handle.pending_checkpoint.lock().await.is_some()
            } else {
                false
            };
            if !has_pending {
                self.active_runs.write().await.remove(run_id);
            }
        }
    }

    pub(super) async fn finalize_agent_loop_run_result(
        &self,
        run_id: &str,
        mut state: RunExecutionState,
        error: Option<&ApplicationError>,
        presentation: Option<Value>,
    ) -> Result<(), ApplicationError> {
        let handle = self.active_run_handle(run_id).await?;
        match handle.scheduler.stop_and_join().await {
            Ok(children) => state.children.extend(children),
            Err(error) => state.blocked_reason = Some(error.to_string()),
        }
        state.guidance = handle.guidance_mailbox.close_and_drain().await;
        self.close_model_sessions_after_run(run_id).await?;
        self.clear_pending_host_requests_for_run(run_id).await;
        let (status, invocation_status, level, event_type, payload) = match error {
            None => (
                AgentRunStatus::Completed,
                AgentInvocationStatus::Completed,
                AgentRunEventLevel::Info,
                "run_completed",
                Value::Null,
            ),
            Some(ApplicationError::Cancelled(message)) => (
                AgentRunStatus::Cancelled,
                AgentInvocationStatus::Cancelled,
                AgentRunEventLevel::Info,
                "run_cancelled",
                json!({ "message": message }),
            ),
            Some(error) if state.commits.is_empty() => (
                AgentRunStatus::Failed,
                AgentInvocationStatus::Failed,
                AgentRunEventLevel::Error,
                "run_failed",
                run_failure_payload(error),
            ),
            Some(error) => (
                AgentRunStatus::PartialSuccess,
                AgentInvocationStatus::Failed,
                AgentRunEventLevel::Warn,
                "run_partial_success",
                run_partial_success_payload(error, &state.commits),
            ),
        };
        if let Some(frame) = &state.foreground {
            if let Some(error) = error
                && let Some(task_id) = frame.prepared.delegation_task_id.as_deref()
            {
                let task_status = if invocation_status == AgentInvocationStatus::Cancelled {
                    tt_domain::models::agent::AgentTaskStatus::Cancelled
                } else {
                    tt_domain::models::agent::AgentTaskStatus::Failed
                };
                self.transition_child_task(
                    run_id,
                    task_id,
                    task_status,
                    None,
                    Some(error.to_string()),
                )
                .await?;
            }
            self.finish_invocation(run_id, &frame.prepared.invocation.id, invocation_status)
                .await?;
        }
        let _publication = self.run_lifecycle_lock.lock().await;
        let mut run = self.run_repository.load_run(run_id).await?;
        run.status = status;
        run.updated_at = chrono::Utc::now();
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "status_changed",
            json!({ "status": status }),
        )
        .await?;
        // The host may observe the terminal event immediately. Hold this slot until the
        // checkpoint is ready so its presentation acknowledgement cannot race capture.
        let mut pending = handle.pending_checkpoint.lock().await;
        let terminal = self.event(run_id, level, event_type, payload).await?;
        *pending = Some(super::checkpoint::RunCheckpoint::new(
            run,
            terminal.seq,
            state,
            handle.stream_override,
            presentation,
        ));
        if !handle.host_presentation {
            self.persist_checkpoint(pending.as_ref().expect("checkpoint was captured"))
                .await?;
            pending.take();
            self.active_runs.write().await.remove(run_id);
        }
        Ok(())
    }

    #[cfg(feature = "test-support")]
    pub async fn execute_agent_loop_run_inner(
        self: &Arc<Self>,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let mut state = RunExecutionState::default();
        let run = self.run_repository.load_run(run_id).await?;
        let scope_order = super::skill_scope::skill_scope_order_for_profile(
            &resolved_profile,
            &run.skill_scope_refs,
        )?;
        let effective_skills = self
            .skill_service
            .resolve_effective_skills(&scope_order, &resolved_profile.skills)
            .await?;
        let (cancel_sender, _) = watch::channel(false);
        self.active_runs.write().await.insert(
            run_id.to_string(),
            Arc::new(super::scheduler::ActiveRunHandle::new(
                self,
                run_id.to_string(),
                cancel_sender,
                None,
                false,
            )),
        );
        let result = self
            .execute_agent_loop_run_body(
                run_id,
                prompt_snapshot,
                request,
                resolved_profile,
                effective_skills,
                &mut state,
                cancel,
            )
            .await;
        self.finalize_agent_loop_run_result(run_id, state, result.as_ref().err(), None)
            .await?;
        result
    }

    async fn close_model_sessions_after_run(&self, run_id: &str) -> Result<(), ApplicationError> {
        let invocations = self.invocation_repository.list_invocations(run_id).await?;
        for invocation in invocations {
            self.model_gateway
                .close_session(&model_session_id(run_id, &invocation.id))
                .await;
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "run bootstrap boundary keeps input snapshot, profile, skills, ledger, and cancellation explicit"
    )]
    async fn execute_agent_loop_run_body(
        &self,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        effective_skills: Vec<SkillIndexEntry>,
        state: &mut RunExecutionState,
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

        let prepared_tools = self
            .prepare_invocation_tools(
                &resolved_profile,
                AgentInvocationExitPolicy::RunFinishAllowed,
                root_invocation.id.as_str(),
            )
            .await?;
        let tool_snapshot = prepared_tools.snapshot;
        let tool_turn = prepared_tools.turn;
        let visible_tools = prepared_tools.model_tools;
        let tool_diagnostics = prepared_tools.diagnostics;
        let tool_snapshot_path = self.persist_tool_snapshot(run_id, &tool_snapshot).await?;
        let mut request = request;
        self.resolve_model_binding(run_id, &resolved_profile, &mut request)
            .await?;
        self.ensure_not_cancelled(cancel)?;
        let request = prepare_agent_tool_request(
            request,
            &visible_tools,
            tool_turn.choice().clone(),
            &run.stable_chat_id,
            run_id,
            root_invocation.id.as_str(),
        )?;
        self.transition_status(run_id, AgentRunStatus::AssemblingContext)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "context_assembled",
            json!({
                "request": request_summary(&request),
                "invocationId": root_invocation.id.as_str(),
                "toolSnapshot": tool_snapshot_summary(&tool_snapshot),
                "toolSnapshotPath": tool_snapshot_path.as_str(),
                "toolTurn": &tool_turn,
                "toolDiagnostics": &tool_diagnostics,
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

        state.foreground = Some(InvocationFrame::new(PreparedInvocation {
            frozen_macros: frozen_macros_from_snapshot(&prompt_snapshot)?,
            invocation: root_invocation,
            delegation_task_id: None,
            profile: resolved_profile,
            tool_snapshot,
            tool_turn,
            request,
            effective_skills,
        }));
        self.execute_active_invocation_chain(state, cancel).await
    }

    async fn execute_active_invocation_chain(
        &self,
        state: &mut RunExecutionState,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        loop {
            let frame = state.foreground.as_mut().ok_or_else(|| {
                ApplicationError::InternalError(
                    "agent.continuation_missing: run has no foreground invocation".to_string(),
                )
            })?;
            let exit = self.run_tool_loop(frame, &mut state.commits, cancel).await?
                .ok_or_else(|| ApplicationError::ValidationError(format!(
                    "agent.max_tool_rounds_exceeded: workspace.finish or agent.handoff was not called within {} rounds",
                    frame.progress.max_rounds)))?;
            match exit {
                AgentLoopExit::Finished => {
                    self.ensure_not_cancelled(cancel)?;
                    let run_id = frame.prepared.invocation.run_id.as_str();
                    state.children.extend(
                        self.active_run_handle(run_id)
                            .await?
                            .scheduler
                            .stop_and_join()
                            .await?,
                    );
                    self.finish_run(
                        run_id,
                        frame.prepared.delegation_task_id.as_deref(),
                        &state.commits,
                        &mut state.published_state,
                        state.previous_published_state_id.as_deref(),
                        cancel,
                    )
                    .await?;
                    return Ok(());
                }
                AgentLoopExit::Transferred {
                    task_id,
                    new_invocation_id,
                } => {
                    if let Some(incoming) = frame.prepared.delegation_task_id.as_deref() {
                        self.transition_child_task(
                            &frame.prepared.invocation.run_id,
                            incoming,
                            tt_domain::models::agent::AgentTaskStatus::Completed,
                            None,
                            None,
                        )
                        .await?;
                    }
                    // The handoff already exists. Keep this frame until its successor is
                    // fully prepared, rather than dispatching the handoff tool again.
                    let prepared = self
                        .prepare_handoff_invocation(
                            &frame.prepared.invocation.run_id,
                            &task_id,
                            &new_invocation_id,
                            cancel,
                        )
                        .await?;
                    state.foreground = Some(InvocationFrame::new(prepared));
                }
            }
        }
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
