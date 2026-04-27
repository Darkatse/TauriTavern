use std::sync::Arc;

use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::watch;
use uuid::Uuid;

use super::AgentRuntimeService;
use super::ids::{validate_stable_chat_id, workspace_id_for_stable_chat_id};
use super::prompt_snapshot::{reject_external_tool_request, request_from_prompt_snapshot};
use crate::application::dto::agent_dto::{
    AgentCancelRunDto, AgentReadEventsDto, AgentReadEventsResultDto, AgentReadWorkspaceFileDto,
    AgentRunHandleDto, AgentStartRunDto, AgentWorkspaceFileDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentRun, AgentRunEventLevel, AgentRunStatus, WorkspacePath};
use crate::domain::repositories::agent_run_repository::AgentRunEventReadQuery;

impl AgentRuntimeService {
    pub async fn start_run(
        self: &Arc<Self>,
        dto: AgentStartRunDto,
    ) -> Result<AgentRunHandleDto, ApplicationError> {
        if dto.options.stream {
            return Err(ApplicationError::ValidationError(
                "agent.phase2b_stream_unsupported: Agent Phase 2B only supports non-streaming model calls"
                    .to_string(),
            ));
        }
        if dto.options.auto_commit {
            return Err(ApplicationError::ValidationError(
                "agent.phase2b_auto_commit_unsupported: commit is owned by the frontend adapter in Agent Phase 2B"
                    .to_string(),
            ));
        }

        let Some(prompt_snapshot) = dto.prompt_snapshot else {
            return Err(ApplicationError::ValidationError(
                "agent.prompt_snapshot_required: Agent tool loop requires a concrete prompt snapshot"
                    .to_string(),
            ));
        };
        let request = request_from_prompt_snapshot(&prompt_snapshot)?;
        reject_external_tool_request(&request.payload)?;

        if dto.generation_type.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.invalid_generation_type: generationType cannot be empty".to_string(),
            ));
        }

        let stable_chat_id = validate_stable_chat_id(&dto.stable_chat_id)?;
        let run_id = format!("run_{}", Uuid::new_v4().simple());
        let workspace_id = workspace_id_for_stable_chat_id(&dto.chat_ref, &stable_chat_id)?;
        let now = Utc::now();
        let run = AgentRun {
            id: run_id.clone(),
            workspace_id: workspace_id.clone(),
            stable_chat_id: stable_chat_id.clone(),
            chat_ref: dto.chat_ref,
            generation_type: dto.generation_type,
            profile_id: dto.profile_id,
            status: AgentRunStatus::Created,
            created_at: now,
            updated_at: now,
        };

        self.run_repository.create_run(&run).await?;
        self.event(
            &run_id,
            AgentRunEventLevel::Info,
            "run_created",
            json!({
                "workspaceId": workspace_id.clone(),
                "stableChatId": stable_chat_id.clone(),
            }),
        )
        .await?;
        if let Some(generation_intent) = dto.generation_intent {
            self.event(
                &run_id,
                AgentRunEventLevel::Info,
                "generation_intent_recorded",
                generation_intent,
            )
            .await?;
        }

        let (cancel_sender, cancel_receiver) = watch::channel(false);
        self.active_runs
            .write()
            .await
            .insert(run_id.clone(), cancel_sender);

        let service = self.clone();
        let background_run_id = run_id.clone();
        tokio::spawn(async move {
            service
                .execute_agent_loop_run(
                    background_run_id,
                    prompt_snapshot,
                    request,
                    cancel_receiver,
                )
                .await;
        });

        Ok(AgentRunHandleDto {
            run_id,
            workspace_id,
            stable_chat_id,
            status: AgentRunStatus::Created,
        })
    }

    pub async fn cancel_run(
        &self,
        dto: AgentCancelRunDto,
    ) -> Result<AgentRunHandleDto, ApplicationError> {
        let run = self.run_repository.load_run(&dto.run_id).await?;
        match run.status {
            AgentRunStatus::Completed | AgentRunStatus::Cancelled | AgentRunStatus::Failed => {
                return Ok(AgentRunHandleDto {
                    run_id: run.id,
                    workspace_id: run.workspace_id,
                    stable_chat_id: run.stable_chat_id,
                    status: run.status,
                });
            }
            _ => {}
        }

        self.event(
            &dto.run_id,
            AgentRunEventLevel::Info,
            "run_cancel_requested",
            Value::Null,
        )
        .await?;

        let sender = self.active_runs.read().await.get(&dto.run_id).cloned();

        let next = match run.status {
            AgentRunStatus::AwaitingCommit | AgentRunStatus::Committing => {
                self.active_runs.write().await.remove(&dto.run_id);
                let cancelled = self
                    .transition_status(&dto.run_id, AgentRunStatus::Cancelled)
                    .await?;
                self.event(
                    &dto.run_id,
                    AgentRunEventLevel::Info,
                    "run_cancelled",
                    Value::Null,
                )
                .await?;
                cancelled
            }
            _ => {
                if let Some(sender) = sender {
                    let _ = sender.send(true);
                    self.transition_status(&dto.run_id, AgentRunStatus::Cancelling)
                        .await?
                } else {
                    let cancelled = self
                        .transition_status(&dto.run_id, AgentRunStatus::Cancelled)
                        .await?;
                    self.event(
                        &dto.run_id,
                        AgentRunEventLevel::Info,
                        "run_cancelled",
                        Value::Null,
                    )
                    .await?;
                    cancelled
                }
            }
        };

        Ok(AgentRunHandleDto {
            run_id: next.id,
            workspace_id: next.workspace_id,
            stable_chat_id: next.stable_chat_id,
            status: next.status,
        })
    }

    pub async fn read_events(
        &self,
        dto: AgentReadEventsDto,
    ) -> Result<AgentReadEventsResultDto, ApplicationError> {
        let events = self
            .run_repository
            .read_events(
                &dto.run_id,
                AgentRunEventReadQuery {
                    after_seq: dto.after_seq,
                    before_seq: dto.before_seq,
                    limit: dto.limit,
                },
            )
            .await?;

        Ok(AgentReadEventsResultDto { events })
    }

    pub async fn read_workspace_file(
        &self,
        dto: AgentReadWorkspaceFileDto,
    ) -> Result<AgentWorkspaceFileDto, ApplicationError> {
        let path = WorkspacePath::parse(dto.path)?;
        let file = self
            .workspace_repository
            .read_text(&dto.run_id, &path)
            .await?;

        Ok(AgentWorkspaceFileDto {
            path: file.path.as_str().to_string(),
            text: file.text,
            bytes: file.bytes,
            sha256: file.sha256,
        })
    }
}
