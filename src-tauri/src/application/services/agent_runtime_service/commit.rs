use serde_json::{Value, json};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::commit_ledger::RunCommitLedger;
use super::{
    AgentCancelReceiver, AgentRuntimeService, HostChatCommitResult, PendingHostChatCommit,
    PendingPersistentStateMetadataUpdate,
};
use crate::application::dto::agent_dto::{
    AgentResolveChatCommitDto, AgentResolvePersistentStateMetadataUpdateDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::{
    AgentToolDispatchOutcome, AgentToolEffect, WORKSPACE_PATH_IS_DIRECTORY_CODE,
    workspace_path_is_directory_message,
};
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentChatCommitMode, AgentRun, AgentRunEventLevel, AgentRunStatus, AgentToolCall,
    AgentToolResult, ArtifactTarget, WorkspacePath, WorkspacePersistentChangeSet,
};
use crate::domain::text_metrics::TextMetrics;

impl AgentRuntimeService {
    pub async fn resolve_chat_commit(
        &self,
        dto: AgentResolveChatCommitDto,
    ) -> Result<(), ApplicationError> {
        let run_id = dto.run_id.trim();
        let commit_id = dto.commit_id.trim();
        if run_id.is_empty() || commit_id.is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.chat_commit_resolve_invalid: runId and commitId are required".to_string(),
            ));
        }

        let pending = {
            let mut commits = self.active_chat_commits.write().await;
            let pending = commits.remove(commit_id).ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.chat_commit_not_pending: commit `{commit_id}` is not awaiting host resolution"
                ))
            })?;
            if pending.run_id != run_id {
                commits.insert(commit_id.to_string(), pending);
                return Err(ApplicationError::ValidationError(format!(
                    "agent.chat_commit_run_mismatch: commit `{commit_id}` belongs to another run"
                )));
            }
            pending
        };

        let result = match dto.error.map(|value| value.trim().to_string()) {
            Some(error) if !error.is_empty() => Err(error),
            _ => Ok(HostChatCommitResult {
                message_id: dto.message_id,
            }),
        };

        pending.sender.send(result).map_err(|_| {
            ApplicationError::ValidationError(format!(
                "agent.chat_commit_resolve_failed: run `{run_id}` is no longer waiting for commit `{commit_id}`"
            ))
        })
    }

    pub async fn resolve_persistent_state_metadata_update(
        &self,
        dto: AgentResolvePersistentStateMetadataUpdateDto,
    ) -> Result<(), ApplicationError> {
        let run_id = dto.run_id.trim();
        let update_id = dto.update_id.trim();
        if run_id.is_empty() || update_id.is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.persistent_state_metadata_update_resolve_invalid: runId and updateId are required"
                    .to_string(),
            ));
        }

        let pending = {
            let mut updates = self.active_persistent_state_metadata_updates.write().await;
            let pending = updates.remove(update_id).ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.persistent_state_metadata_update_not_pending: update `{update_id}` is not awaiting host resolution"
                ))
            })?;
            if pending.run_id != run_id {
                updates.insert(update_id.to_string(), pending);
                return Err(ApplicationError::ValidationError(format!(
                    "agent.persistent_state_metadata_update_run_mismatch: update `{update_id}` belongs to another run"
                )));
            }
            pending
        };

        let result = match dto.error.map(|value| value.trim().to_string()) {
            Some(error) if !error.is_empty() => Err(error),
            _ => Ok(()),
        };

        pending.sender.send(result).map_err(|_| {
            ApplicationError::ValidationError(format!(
                "agent.persistent_state_metadata_update_resolve_failed: run `{run_id}` is no longer waiting for update `{update_id}`"
            ))
        })
    }

    pub(super) async fn perform_host_chat_commit(
        &self,
        run_id: &str,
        call: &AgentToolCall,
        path: WorkspacePath,
        mode: AgentChatCommitMode,
        reason: Option<String>,
        elapsed_ms: u128,
        round: usize,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let file = match self.workspace_repository.read_text(run_id, &path).await {
            Ok(file) => file,
            Err(DomainError::NotFound(message)) => {
                return Ok(recoverable_tool_error(
                    call,
                    "workspace.file_not_found",
                    &message,
                    elapsed_ms,
                ));
            }
            Err(DomainError::WorkspacePathIsDirectory { path }) => {
                return Ok(recoverable_tool_error(
                    call,
                    WORKSPACE_PATH_IS_DIRECTORY_CODE,
                    &workspace_path_is_directory_message(&path),
                    elapsed_ms,
                ));
            }
            Err(error) => return Err(error.into()),
        };
        let manifest = self.workspace_repository.read_manifest(run_id).await?;
        if manifest.artifacts.iter().any(|artifact| {
            matches!(artifact.target, ArtifactTarget::MessageBody)
                && artifact.required
                && artifact.path == path.as_str()
        }) && file.text.trim().is_empty()
        {
            return Ok(recoverable_tool_error(
                call,
                "workspace.required_artifact_empty",
                &format!("{} is empty", path.as_str()),
                elapsed_ms,
            ));
        }

        let run = self.run_repository.load_run(run_id).await?;
        let commit_id = format!("commit_{}", Uuid::new_v4().simple());
        let started = self
            .event(
                run_id,
                AgentRunEventLevel::Info,
                "chat_commit_started",
                json!({
                    "commitId": commit_id.as_str(),
                    "callId": call.id.as_str(),
                    "path": path.as_str(),
                    "mode": mode,
                    "reason": reason.as_deref(),
                }),
            )
            .await?;

        self.transition_status(run_id, AgentRunStatus::CreatingCheckpoint)
            .await?;
        let checkpoint = self
            .checkpoint_repository
            .create_checkpoint(run_id, "chat_commit", started.seq, &[path.clone()])
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "checkpoint_created",
            json!({
                "checkpointId": checkpoint.id.as_str(),
                "reason": "chat_commit",
            }),
        )
        .await?;

        let (sender, receiver) = oneshot::channel();
        let previous = self.active_chat_commits.write().await.insert(
            commit_id.clone(),
            PendingHostChatCommit {
                run_id: run_id.to_string(),
                sender,
            },
        );
        if previous.is_some() {
            return Err(ApplicationError::InternalError(format!(
                "agent.chat_commit_id_collision: duplicate commit id `{commit_id}`"
            )));
        }

        self.transition_status(run_id, AgentRunStatus::AwaitingHostCommit)
            .await?;
        let file_metrics = TextMetrics::from_text(&file.text);
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "chat_commit_requested",
            json!({
                "commitId": commit_id,
                "callId": call.id.as_str(),
                "runId": run.id.as_str(),
                "workspaceId": run.workspace_id.as_str(),
                "stableChatId": run.stable_chat_id.as_str(),
                "chatRef": &run.chat_ref,
                "generationType": run.generation_type.as_str(),
                "profileId": run.profile_id.as_ref(),
                "persistBaseStateId": run.persist_base_state_id.as_deref(),
                "path": file.path.as_str(),
                "mode": mode,
                "reason": reason.as_deref(),
                "chars": file_metrics.chars,
                "words": file_metrics.words,
                "sha256": file.sha256.as_str(),
                "checkpointId": checkpoint.id.as_str(),
            }),
        )
        .await?;

        let host_result = tokio::select! {
            result = receiver => {
                result.map_err(|_| ApplicationError::InternalError(format!(
                    "agent.chat_commit_channel_closed: host commit `{commit_id}` closed before resolution"
                )))?
            }
            changed = cancel.changed() => {
                let _ = changed;
                self.active_chat_commits.write().await.remove(&commit_id);
                self.ensure_not_cancelled(cancel)?;
                return Err(ApplicationError::Cancelled(
                    "Agent run cancelled while awaiting host chat commit".to_string(),
                ));
            }
        };

        match host_result {
            Ok(result) => {
                commit_ledger.record(&path, mode, result.message_id.clone(), round);
                self.transition_status(run_id, AgentRunStatus::DispatchingTool)
                    .await?;
                self.event(
                    run_id,
                    AgentRunEventLevel::Info,
                    "chat_commit_completed",
                    json!({
                        "commitId": commit_id,
                        "callId": call.id.as_str(),
                        "path": path.as_str(),
                        "mode": mode,
                        "messageId": result.message_id.as_deref(),
                    }),
                )
                .await?;

                Ok(AgentToolDispatchOutcome {
                    result: AgentToolResult {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: format!(
                            "Committed {} to the current chat message with mode {:?}. \
                             You may continue editing and commit again if needed. When all intended \
                             commits are complete, call workspace_finish to end the run. Do not use \
                             plain text as the final answer; the run must finish through \
                             workspace_finish.",
                            path.as_str(),
                            mode
                        ),
                        structured: json!({
                            "path": path.as_str(),
                            "mode": mode,
                            "messageId": result.message_id.as_deref(),
                            "chars": file_metrics.chars,
                            "words": file_metrics.words,
                        }),
                        is_error: false,
                        error_code: None,
                        resource_refs: vec![path.as_str().to_string()],
                    },
                    effect: AgentToolEffect::ChatCommitted {
                        path,
                        mode,
                        message_id: result.message_id,
                    },
                    elapsed_ms,
                })
            }
            Err(message) => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "chat_commit_failed",
                    json!({
                        "commitId": commit_id,
                        "callId": call.id.as_str(),
                        "path": path.as_str(),
                        "mode": mode,
                        "message": message,
                    }),
                )
                .await?;
                Err(ApplicationError::ValidationError(format!(
                    "agent.chat_commit_failed: {message}"
                )))
            }
        }
    }

    pub(super) async fn finish_run(
        &self,
        run_id: &str,
        commit_ledger: &RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        self.transition_status(run_id, AgentRunStatus::Finishing)
            .await?;
        let run = self.run_repository.load_run(run_id).await?;

        let persistent_changes = match self
            .workspace_repository
            .commit_persistent_changes(run_id)
            .await
        {
            Ok(changes) => changes,
            Err(error) => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "persistent_changes_commit_failed",
                    json!({ "message": error.to_string() }),
                )
                .await?;
                return Err(error.into());
            }
        };
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "persistent_changes_committed",
            json!({
                "stateId": persistent_changes.state_id,
                "baseStateId": persistent_changes.base_state_id,
                "changeCount": persistent_changes.changes.len(),
                "changes": persistent_change_payloads(&persistent_changes),
            }),
        )
        .await?;

        self.request_persistent_state_metadata_update(
            &run,
            &persistent_changes,
            commit_ledger,
            cancel,
        )
        .await?;

        self.transition_status(run_id, AgentRunStatus::Completed)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "run_completed",
            Value::Null,
        )
        .await?;
        self.active_runs.write().await.remove(run_id);
        self.clear_pending_host_requests_for_run(run_id).await;

        Ok(())
    }

    async fn request_persistent_state_metadata_update(
        &self,
        run: &AgentRun,
        persistent_changes: &WorkspacePersistentChangeSet,
        commit_ledger: &RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        if commit_ledger.is_empty() {
            return Ok(());
        }
        let message_id = commit_ledger.latest_message_id().ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.persistent_state_message_missing: host chat commit did not return a messageId"
                    .to_string(),
            )
        })?;
        let update_id = format!("persist_update_{}", Uuid::new_v4().simple());
        let (sender, receiver) = oneshot::channel();
        let previous = self
            .active_persistent_state_metadata_updates
            .write()
            .await
            .insert(
                update_id.clone(),
                PendingPersistentStateMetadataUpdate {
                    run_id: run.id.clone(),
                    sender,
                },
            );
        if previous.is_some() {
            return Err(ApplicationError::InternalError(format!(
                "agent.persistent_state_metadata_update_id_collision: duplicate update id `{update_id}`"
            )));
        }

        self.event(
            run.id.as_str(),
            AgentRunEventLevel::Info,
            "persistent_state_metadata_update_requested",
            json!({
                "updateId": update_id.as_str(),
                "runId": run.id.as_str(),
                "workspaceId": run.workspace_id.as_str(),
                "stableChatId": run.stable_chat_id.as_str(),
                "chatRef": &run.chat_ref,
                "generationType": run.generation_type.as_str(),
                "profileId": run.profile_id.as_ref(),
                "messageId": message_id,
                "stateId": persistent_changes.state_id.as_str(),
                "baseStateId": persistent_changes.base_state_id.as_deref(),
                "changeCount": persistent_changes.changes.len(),
                "changes": persistent_change_payloads(persistent_changes),
            }),
        )
        .await?;

        let host_result = tokio::select! {
            result = receiver => {
                result.map_err(|_| ApplicationError::InternalError(format!(
                    "agent.persistent_state_metadata_update_channel_closed: host update `{update_id}` closed before resolution"
                )))?
            }
            changed = cancel.changed() => {
                let _ = changed;
                self.active_persistent_state_metadata_updates
                    .write()
                    .await
                    .remove(&update_id);
                self.ensure_not_cancelled(cancel)?;
                return Err(ApplicationError::Cancelled(
                    "Agent run cancelled while awaiting persistent state metadata update".to_string(),
                ));
            }
        };

        match host_result {
            Ok(()) => {
                self.event(
                    run.id.as_str(),
                    AgentRunEventLevel::Info,
                    "persistent_state_metadata_updated",
                    json!({
                        "updateId": update_id,
                        "messageId": message_id,
                        "stateId": persistent_changes.state_id.as_str(),
                    }),
                )
                .await?;
                Ok(())
            }
            Err(message) => {
                self.event(
                    run.id.as_str(),
                    AgentRunEventLevel::Error,
                    "persistent_state_metadata_update_failed",
                    json!({
                        "updateId": update_id,
                        "messageId": message_id,
                        "stateId": persistent_changes.state_id.as_str(),
                        "message": message,
                    }),
                )
                .await?;
                Err(ApplicationError::ValidationError(format!(
                    "agent.persistent_state_metadata_update_failed: {message}"
                )))
            }
        }
    }

    pub(super) async fn clear_pending_host_requests_for_run(&self, run_id: &str) {
        self.active_chat_commits
            .write()
            .await
            .retain(|_, pending| pending.run_id != run_id);
        self.active_persistent_state_metadata_updates
            .write()
            .await
            .retain(|_, pending| pending.run_id != run_id);
    }
}

fn persistent_change_payloads(changes: &WorkspacePersistentChangeSet) -> Vec<Value> {
    changes
        .changes
        .iter()
        .map(|change| {
            json!({
                "path": change.path.as_str(),
                "kind": change.kind,
                "sha256": change.sha256.as_str(),
            })
        })
        .collect()
}

fn recoverable_tool_error(
    call: &AgentToolCall,
    code: &str,
    message: &str,
    elapsed_ms: u128,
) -> AgentToolDispatchOutcome {
    AgentToolDispatchOutcome {
        result: AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content: message.to_string(),
            structured: json!({
                "error": {
                    "code": code,
                    "message": message,
                }
            }),
            is_error: true,
            error_code: Some(code.to_string()),
            resource_refs: Vec::new(),
        },
        effect: AgentToolEffect::None,
        elapsed_ms,
    }
}
