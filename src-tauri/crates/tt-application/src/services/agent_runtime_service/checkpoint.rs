use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

use super::AgentRuntimeService;
use super::continuation::{InvocationFrame, InvocationStep, RunExecutionState};
use super::loop_runner::AgentLoopExit;
use super::prompt_snapshot::frozen_macros_from_snapshot;
use super::scheduler::ActiveRunHandle;
use crate::dto::agent_dto::{
    AgentFinishRunPresentationDto, AgentReadRunCheckpointDto, AgentReadRunCheckpointResultDto,
    AgentResumeRunDto, AgentRunHandleDto,
};
use crate::errors::ApplicationError;
use crate::services::agent_identity::{validate_stable_chat_id, workspace_id_for_stable_chat_id};
use crate::services::agent_model_gateway::reset_transport_for_resume;
use tt_domain::models::agent::{
    AgentInvocationStatus, AgentRun, AgentRunEventLevel, AgentRunStatus, AgentTaskStatus,
    WorkspacePath,
};
use tt_domain::models::tool::ToolTurnContract;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RunCheckpoint {
    schema_version: u32,
    run: AgentRun,
    terminal_seq: u64,
    state: RunExecutionState,
    presentation: Option<Value>,
    stream_override: Option<bool>,
}

impl RunCheckpoint {
    pub(super) fn new(
        run: AgentRun,
        terminal_seq: u64,
        mut state: RunExecutionState,
        stream_override: Option<bool>,
        presentation: Option<Value>,
    ) -> Self {
        if state.foreground.is_none() && state.blocked_reason.is_none() {
            state.blocked_reason =
                Some("Run initialization did not produce an invocation to resume.".to_string());
        }
        Self {
            schema_version: 1,
            run,
            terminal_seq,
            state,
            presentation,
            stream_override,
        }
    }

    pub(super) fn handle(&self) -> AgentRunHandleDto {
        run_handle(&self.run, None)
    }

    fn blocked_reason(&self) -> Option<&str> {
        self.state.blocked_reason.as_deref().or_else(|| {
            self.state
                .foreground
                .iter()
                .chain(&self.state.children)
                .find_map(|frame| frame.progress.blocked_reason.as_deref())
        })
    }

    fn next_step(&self) -> &'static str {
        if self.run.status == AgentRunStatus::Completed {
            return "finished";
        }
        match self
            .state
            .foreground
            .as_ref()
            .map(|frame| &frame.progress.step)
        {
            Some(InvocationStep::Model) => "model",
            Some(InvocationStep::Tools(_)) => "tools",
            Some(InvocationStep::Exited(AgentLoopExit::Transferred { .. })) => "handoff",
            Some(InvocationStep::Exited(AgentLoopExit::Finished)) => "finalize",
            None => "unavailable",
        }
    }
}

impl AgentRuntimeService {
    pub(super) async fn persist_checkpoint(
        &self,
        checkpoint: &RunCheckpoint,
    ) -> Result<(), ApplicationError> {
        let bytes = serde_json::to_vec(checkpoint)
            .map_err(|error| invalid(format!("checkpoint serialization failed: {error}")))?;
        self.run_repository
            .save_run_checkpoint(&checkpoint.run.id, &bytes)
            .await?;
        // Keep the persisted Run nonterminal while the host finishes presentation.
        // Sync must not export the previous checkpoint as this execution's final state.
        self.run_repository.save_run(&checkpoint.run).await?;
        Ok(())
    }

    async fn load_checkpoint(&self, run_id: &str) -> Result<RunCheckpoint, ApplicationError> {
        if self.active_runs.read().await.contains_key(run_id) {
            return Err(invalid("run is still active or finishing its presentation"));
        }
        let bytes = self
            .run_repository
            .load_run_checkpoint(run_id)
            .await?
            .ok_or_else(|| {
                invalid(
                    "this run has no checkpoint (it may predate checkpoints or have been cleaned)",
                )
            })?;
        let checkpoint: RunCheckpoint = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("checkpoint cannot be decoded: {error}")))?;
        if checkpoint.schema_version != 1 || checkpoint.run.id != run_id {
            return Err(invalid("checkpoint version or run identity does not match"));
        }
        let run = self.run_repository.load_run(run_id).await?;
        if !run.status.is_terminal()
            || run.updated_at != checkpoint.run.updated_at
            || run.status != checkpoint.run.status
        {
            return Err(invalid(
                "the run has changed since this checkpoint was saved",
            ));
        }
        Ok(checkpoint)
    }

    pub async fn read_run_checkpoint(
        &self,
        dto: AgentReadRunCheckpointDto,
    ) -> Result<AgentReadRunCheckpointResultDto, ApplicationError> {
        let checkpoint = self.load_checkpoint(&dto.run_id).await?;
        let frame = checkpoint.state.foreground.as_ref();
        Ok(AgentReadRunCheckpointResultDto {
            next_step: checkpoint.next_step().to_string(),
            blocked_reason: checkpoint.blocked_reason().map(str::to_string),
            round: frame.map_or(0, |frame| frame.progress.round),
            max_rounds: frame.map_or(0, |frame| frame.progress.max_rounds),
            run: run_handle(&checkpoint.run, None),
            terminal_seq: checkpoint.terminal_seq,
            presentation: checkpoint.presentation,
        })
    }

    pub async fn finish_run_presentation(
        &self,
        dto: AgentFinishRunPresentationDto,
    ) -> Result<(), ApplicationError> {
        let _publication = self.run_lifecycle_lock.lock().await;
        let handle = self.active_run_handle(&dto.run_id).await?;
        let mut pending = handle.pending_checkpoint.lock().await;
        let checkpoint = pending
            .as_mut()
            .ok_or_else(|| invalid("runtime has no pending final checkpoint"))?;
        if checkpoint.terminal_seq != dto.terminal_seq {
            return Err(invalid(
                "presentation acknowledgement belongs to a different execution",
            ));
        }
        checkpoint.presentation = dto.presentation;
        self.persist_checkpoint(checkpoint).await?;
        pending.take();
        self.active_runs.write().await.remove(&dto.run_id);
        Ok(())
    }

    pub async fn resume_run(
        self: &Arc<Self>,
        dto: AgentResumeRunDto,
    ) -> Result<AgentRunHandleDto, ApplicationError> {
        let _admission = self.run_lifecycle_lock.lock().await;
        let mut checkpoint = self.load_checkpoint(&dto.run_id).await?;
        if checkpoint.terminal_seq != dto.expected_terminal_seq {
            return Err(invalid(
                "a newer checkpoint exists; read it before resuming",
            ));
        }
        if checkpoint.run.status == AgentRunStatus::Completed {
            return Err(invalid("completed runs cannot be resumed"));
        }
        if let Some(reason) = checkpoint.blocked_reason() {
            return Err(invalid(reason));
        }
        let stable_chat_id = validate_stable_chat_id(&dto.stable_chat_id)?;
        if stable_chat_id != checkpoint.run.stable_chat_id
            || workspace_id_for_stable_chat_id(&dto.chat_ref, &stable_chat_id)?
                != checkpoint.run.workspace_id
        {
            return Err(invalid("resume must target the original chat workspace"));
        }
        // Rehydrate shared frozen context once. The workspace and invocation snapshots
        // remain the originals; no profile resolution or prompt assembly occurs here.
        self.workspace_repository.read_manifest(&dto.run_id).await?;
        let snapshot = self
            .workspace_repository
            .read_text(
                &dto.run_id,
                &WorkspacePath::parse("input/prompt_snapshot.json")?,
            )
            .await?;
        let snapshot: Value = serde_json::from_str(&snapshot.text).map_err(|error| {
            invalid(format!("frozen prompt snapshot cannot be decoded: {error}"))
        })?;
        let macros = frozen_macros_from_snapshot(&snapshot)?;
        for frame in checkpoint
            .state
            .foreground
            .iter_mut()
            .chain(&mut checkpoint.state.children)
        {
            hydrate_frame(frame, &dto.run_id, dto.additional_rounds, &macros)?;
        }
        let foreground = checkpoint
            .state
            .foreground
            .as_ref()
            .ok_or_else(|| invalid("foreground invocation is missing"))?;
        if matches!(foreground.progress.step, InvocationStep::Model)
            && foreground.progress.round > foreground.progress.max_rounds
        {
            return Err(invalid(
                "round budget is exhausted; explicitly add rounds to resume",
            ));
        }
        self.run_repository
            .reset_event_sequence(&dto.run_id)
            .await?;
        checkpoint.run.chat_ref = dto.chat_ref;
        checkpoint.run.status = AgentRunStatus::AssemblingContext;
        checkpoint.run.updated_at = Utc::now();
        let (cancel_sender, cancel_receiver) = watch::channel(false);
        let mut handle = ActiveRunHandle::new(
            self,
            dto.run_id.clone(),
            cancel_sender,
            checkpoint.stream_override,
            dto.host_presentation,
        );
        for item in checkpoint.state.guidance.drain(..) {
            handle.guidance_mailbox.enqueue(item).await?;
        }
        let admission = async {
            // Invalidate the old checkpoint durably before resumed execution can write.
            self.run_repository.save_run(&checkpoint.run).await?;
            self.event(
                &dto.run_id,
                AgentRunEventLevel::Info,
                "run_resumed",
                json!({
                    "checkpointTerminalSeq": checkpoint.terminal_seq,
                    "additionalRounds": dto.additional_rounds,
                    "invocationId": foreground_id(&checkpoint.state),
                }),
            )
            .await
        }
        .await;
        let resumed = match admission {
            Ok(event) => event,
            Err(error) => {
                // No bridge was attached. Preserve the original presentation while
                // ending this failed admission through the ordinary checkpoint path.
                handle.host_presentation = false;
                self.active_runs
                    .write()
                    .await
                    .insert(dto.run_id.clone(), Arc::new(handle));
                drop(_admission);
                self.finalize_agent_loop_run_result(
                    &dto.run_id,
                    checkpoint.state,
                    Some(&error),
                    checkpoint.presentation,
                )
                .await?;
                return Err(error);
            }
        };
        self.active_runs
            .write()
            .await
            .insert(dto.run_id.clone(), Arc::new(handle));
        let result = run_handle(&checkpoint.run, Some(resumed.seq - 1));
        let service = Arc::clone(self);
        tokio::spawn(async move {
            service
                .execute_resumed_run(dto.run_id, checkpoint.state, cancel_receiver)
                .await;
        });
        Ok(result)
    }

    pub(super) async fn reopen_invocation(
        &self,
        frame: &mut InvocationFrame,
    ) -> Result<(), ApplicationError> {
        let invocation = &mut frame.prepared.invocation;
        invocation.status = AgentInvocationStatus::Running;
        invocation.updated_at = Utc::now();
        self.invocation_repository
            .save_invocation(invocation)
            .await?;
        if let Some(task_id) = &frame.prepared.delegation_task_id {
            let mut task = self
                .invocation_repository
                .load_task(&invocation.run_id, task_id)
                .await?;
            task.status = AgentTaskStatus::Running;
            task.error = None;
            task.updated_at = Utc::now();
            self.invocation_repository.save_task(&task).await?;
        }
        Ok(())
    }
}

fn hydrate_frame(
    frame: &mut InvocationFrame,
    run_id: &str,
    additional_rounds: usize,
    macros: &Arc<tt_domain::frozen_macros::FrozenMacros>,
) -> Result<(), ApplicationError> {
    let prepared = &mut frame.prepared;
    if prepared.invocation.run_id != run_id
        || ToolTurnContract::all(&prepared.tool_snapshot, prepared.tool_turn.choice().clone())?
            != prepared.tool_turn
    {
        return Err(invalid(
            "invocation identity or tool contract does not match the checkpoint",
        ));
    }
    if frame.progress.round == 0
        || matches!(&frame.progress.step, InvocationStep::Tools(turn) if turn.next_call > turn.calls.len())
    {
        return Err(invalid("checkpoint execution cursor is invalid"));
    }
    frame.progress.max_rounds = frame
        .progress
        .max_rounds
        .checked_add(additional_rounds)
        .ok_or_else(|| invalid("additional round budget is too large"))?;
    prepared.frozen_macros = Arc::clone(macros);
    frame.progress.session.frozen_macros = Arc::clone(macros);
    frame.progress.session.effective_skills = prepared.effective_skills.clone();
    reset_transport_for_resume(&mut prepared.request);
    Ok(())
}

fn foreground_id(state: &RunExecutionState) -> Option<&str> {
    state
        .foreground
        .as_ref()
        .map(|frame| frame.prepared.invocation.id.as_str())
}

fn run_handle(run: &AgentRun, after_seq: Option<u64>) -> AgentRunHandleDto {
    AgentRunHandleDto {
        run_id: run.id.clone(),
        workspace_id: run.workspace_id.clone(),
        stable_chat_id: run.stable_chat_id.clone(),
        generation_type: run.generation_type.clone(),
        status: run.status,
        after_seq,
    }
}

fn invalid(reason: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::ValidationError(format!("agent.resume_unavailable: {reason}"))
}
