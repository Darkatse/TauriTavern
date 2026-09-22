use std::sync::Arc;
use tt_ports::workspace_fs::WorkspaceWriteGuard;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

use super::AgentRuntimeService;
use super::continuation::{InvocationFrame, InvocationStep, RunExecutionState};
use super::guidance::AgentGuidanceItem;
use super::loop_runner::AgentLoopExit;
use super::prompt_snapshot::runtime_context_from_snapshot;
use super::revision::PREVIOUS_OUTPUT_PATH;
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

mod legacy;

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
            schema_version: 2,
            run,
            terminal_seq,
            state,
            presentation,
            stream_override,
        }
    }

    pub(super) fn handle(&self) -> Result<AgentRunHandleDto, ApplicationError> {
        run_handle(&self.run, None)
    }

    fn blocked_reason(&self) -> Option<&str> {
        if self.schema_version == 1 && self.run.status != AgentRunStatus::Completed {
            return Some(legacy::REVISION_ONLY);
        }
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
            Some(InvocationStep::Exited(AgentLoopExit::Finished | AgentLoopExit::Replied)) => {
                "finalize"
            }
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
        // Publish matching Run metadata only after its checkpoint is saved.
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
        let mut value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("checkpoint cannot be decoded: {error}")))?;
        match value.get("schemaVersion").and_then(Value::as_u64) {
            Some(1) => legacy::prepare_for_read(&mut value),
            Some(2) => {}
            _ => return Err(invalid("unsupported checkpoint version; start a new run")),
        }
        tt_contracts::agent_run_record::canonicalize_agent_run_record(
            value
                .get_mut("run")
                .ok_or_else(|| invalid("checkpoint is missing its run"))?,
        )?;
        let checkpoint: RunCheckpoint = serde_json::from_value(value)
            .map_err(|error| invalid(format!("checkpoint cannot be decoded: {error}")))?;
        if checkpoint.run.id != run_id {
            return Err(invalid("checkpoint run identity does not match"));
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
        self.run_repository
            .load_run(&dto.run_id)
            .await?
            .chat_target()?;
        let checkpoint = self.load_checkpoint(&dto.run_id).await?;
        let frame = checkpoint.state.foreground.as_ref();
        Ok(AgentReadRunCheckpointResultDto {
            next_step: checkpoint.next_step().to_string(),
            blocked_reason: checkpoint.blocked_reason().map(str::to_string),
            round: frame.map_or(0, |frame| frame.progress.round),
            max_rounds: frame.map_or(0, |frame| frame.progress.max_rounds),
            run: run_handle(&checkpoint.run, None)?,
            terminal_seq: checkpoint.terminal_seq,
            presentation: checkpoint.presentation,
        })
    }

    pub async fn finish_run_presentation(
        &self,
        dto: AgentFinishRunPresentationDto,
    ) -> Result<(), ApplicationError> {
        let _publication = self.run_lifecycle_lock.lock().await;
        self.run_repository
            .load_run(&dto.run_id)
            .await?
            .chat_target()?;
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
        self.run_repository
            .load_run(&dto.run_id)
            .await?
            .chat_target()?;
        let mut checkpoint = self.load_checkpoint(&dto.run_id).await?;
        if checkpoint.terminal_seq != dto.expected_terminal_seq {
            return Err(invalid(
                "a newer checkpoint exists; read it before resuming",
            ));
        }
        if checkpoint.schema_version == 1
            && (dto.revision.is_none() || checkpoint.run.status != AgentRunStatus::Completed)
        {
            return Err(invalid(legacy::REVISION_ONLY));
        }
        if dto.revision.is_some() && checkpoint.run.status != AgentRunStatus::Completed {
            return Err(invalid(
                "continue the unfinished run before revising its output",
            ));
        }
        if dto.revision.is_none() && checkpoint.run.status == AgentRunStatus::Completed {
            return Err(invalid("completed runs cannot be resumed"));
        }
        if let Some(reason) = checkpoint.blocked_reason() {
            return Err(invalid(reason));
        }
        let stable_chat_id = validate_stable_chat_id(&dto.stable_chat_id)?;
        if stable_chat_id != checkpoint.run.chat_target()?.stable_chat_id
            || workspace_id_for_stable_chat_id(&dto.chat_ref, &stable_chat_id)?
                != checkpoint.run.workspace_id
        {
            return Err(invalid("resume must target the original chat workspace"));
        }
        let revision_guidance = dto
            .revision
            .as_ref()
            .map(|revision| AgentGuidanceItem::new(&revision.guidance, None))
            .transpose()?;
        let migrating_legacy_revision = checkpoint.schema_version == 1;
        if let Some(revision) = &dto.revision {
            checkpoint.state.begin_output_revision()?;
            if migrating_legacy_revision {
                let frame = checkpoint
                    .state
                    .foreground
                    .as_mut()
                    .expect("revision foreground");
                let agents = self
                    .agent_catalog(&frame.prepared.profile, &frame.prepared.request.tools)
                    .await?;
                legacy::migrate_revision(&mut frame.prepared, &agents)?;
                checkpoint.schema_version = 2;
            }
            self.workspace_files(&dto.run_id)
                .await?
                .write_text(
                    &WorkspacePath::parse(PREVIOUS_OUTPUT_PATH)?,
                    &revision.previous_output,
                    WorkspaceWriteGuard::Unchecked,
                )
                .await?;
        }
        // Rehydrate the original frozen context once. Revision migration uses saved
        // bindings too; the caller's Profile and prompt are not resolved again.
        self.workspace_repository.read_manifest(&dto.run_id).await?;
        let snapshot = self
            .workspace_files(&dto.run_id)
            .await?
            .read_text(&WorkspacePath::parse("input/prompt_snapshot.json")?)
            .await?;
        let snapshot: Value = serde_json::from_str(&snapshot.text).map_err(|error| {
            invalid(format!("frozen prompt snapshot cannot be decoded: {error}"))
        })?;
        let context = runtime_context_from_snapshot(&snapshot)?;
        for frame in checkpoint
            .state
            .foreground
            .iter_mut()
            .chain(&mut checkpoint.state.children)
        {
            hydrate_frame(frame, &dto.run_id, dto.additional_rounds, &context)?;
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
        checkpoint.run.chat_target_mut()?.chat_ref = dto.chat_ref;
        checkpoint.run.status = AgentRunStatus::AssemblingContext;
        checkpoint.run.updated_at = Utc::now();
        let (cancel_sender, cancel_receiver) = watch::channel(false);
        let mut handle = ActiveRunHandle::new(
            self,
            &checkpoint.run,
            self.workspace_repository
                .open_filesystem(&dto.run_id)
                .await?,
            cancel_sender,
            checkpoint.stream_override,
            dto.host_presentation,
        );
        for item in checkpoint.state.guidance.drain(..) {
            handle.guidance_mailbox.enqueue(item).await?;
        }
        if let Some(item) = &revision_guidance {
            handle.guidance_mailbox.enqueue(item.clone()).await?;
        }
        let admission = async {
            // Invalidate the old checkpoint durably before resumed execution can write.
            self.run_repository.save_run(&checkpoint.run).await?;
            let resumed = self
                .event(
                    &dto.run_id,
                    AgentRunEventLevel::Info,
                    "run_resumed",
                    json!({
                        "checkpointTerminalSeq": checkpoint.terminal_seq,
                        "additionalRounds": dto.additional_rounds,
                        "invocationId": foreground_id(&checkpoint.state),
                        "revision": dto.revision.is_some(),
                    }),
                )
                .await?;
            if dto.revision.is_some() {
                let frame = checkpoint
                    .state
                    .foreground
                    .as_ref()
                    .expect("revision foreground");
                if migrating_legacy_revision {
                    self.persist_tool_snapshot(&dto.run_id, &frame.prepared.tool_snapshot)
                        .await?;
                }
                self.save_revision_invocation(frame).await?;
            }
            if let Some(item) = &revision_guidance {
                self.record_guidance_submission(
                    &dto.run_id,
                    foreground_id(&checkpoint.state).expect("revision foreground"),
                    item,
                )
                .await?;
            }
            Ok::<_, ApplicationError>(resumed)
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
        let result = run_handle(&checkpoint.run, Some(resumed.seq - 1))?;
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
        self.event(
            &invocation.run_id,
            AgentRunEventLevel::Info,
            "agent_invocation_started",
            json!({
                "invocationId": invocation.id,
                "profileId": invocation.profile_id,
                "kind": invocation.kind,
                "status": invocation.status,
            }),
        )
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
    context: &Arc<tt_ports::workspace_shell::WorkspaceShellContext>,
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
    prepared.runtime_context = Arc::clone(context);
    frame.progress.session.runtime_context = Arc::clone(context);
    frame.progress.session.effective_skills = prepared.effective_skills.clone().into();
    reset_transport_for_resume(&mut prepared.request);
    Ok(())
}

fn foreground_id(state: &RunExecutionState) -> Option<&str> {
    state
        .foreground
        .as_ref()
        .map(|frame| frame.prepared.invocation.id.as_str())
}

fn run_handle(
    run: &AgentRun,
    after_seq: Option<u64>,
) -> Result<AgentRunHandleDto, ApplicationError> {
    let chat = run.chat_target()?;
    Ok(AgentRunHandleDto {
        run_id: run.id.clone(),
        workspace_id: run.workspace_id.clone(),
        stable_chat_id: chat.stable_chat_id.clone(),
        generation_type: chat.generation_type.clone(),
        status: run.status,
        after_seq,
    })
}

fn invalid(reason: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::ValidationError(format!("agent.resume_unavailable: {reason}"))
}
