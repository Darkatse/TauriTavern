use serde_json::{Value, json};

use super::AgentRuntimeService;
use crate::application::dto::agent_dto::{
    AgentCommitDraftDto, AgentCommitMessageDto, AgentCommitResultDto, AgentFinalizeCommitDto,
    AgentPrepareCommitDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AgentRunEventLevel, AgentRunStatus, ArtifactTarget, WorkspacePath,
};

impl AgentRuntimeService {
    pub async fn prepare_commit(
        &self,
        dto: AgentPrepareCommitDto,
    ) -> Result<AgentCommitDraftDto, ApplicationError> {
        let run = self.run_repository.load_run(&dto.run_id).await?;
        match run.status {
            AgentRunStatus::AwaitingCommit | AgentRunStatus::Committing => {}
            status => {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.invalid_commit_state: expected awaiting_commit or committing, got {:?}",
                    status
                )));
            }
        }
        let persistent_changes = match self
            .workspace_repository
            .prepare_persistent_changes(&dto.run_id)
            .await
        {
            Ok(changes) => changes,
            Err(error) => {
                self.event(
                    &dto.run_id,
                    AgentRunEventLevel::Error,
                    "persistent_changes_prepare_failed",
                    json!({ "message": error.to_string() }),
                )
                .await?;
                return Err(error.into());
            }
        };
        let run = if run.status == AgentRunStatus::AwaitingCommit {
            self.transition_status(&dto.run_id, AgentRunStatus::Committing)
                .await?
        } else {
            run
        };
        let commit_event = self
            .event(
                &dto.run_id,
                AgentRunEventLevel::Info,
                "commit_started",
                Value::Null,
            )
            .await?;

        let manifest = self.workspace_repository.read_manifest(&dto.run_id).await?;
        let message_artifact = manifest
            .artifacts
            .iter()
            .find(|artifact| matches!(artifact.target, ArtifactTarget::MessageBody))
            .ok_or_else(|| {
                ApplicationError::ValidationError(
                    "workspace.message_body_artifact_missing: manifest does not declare a message body artifact"
                        .to_string(),
                )
            })?;
        let artifact_path = WorkspacePath::parse(&message_artifact.path)?;
        let file = self
            .workspace_repository
            .read_text(&dto.run_id, &artifact_path)
            .await?;
        if message_artifact.required && file.text.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "workspace.required_artifact_empty: output/main.md is empty".to_string(),
            ));
        }

        let required_paths = manifest
            .artifacts
            .iter()
            .filter(|artifact| artifact.required)
            .map(|artifact| WorkspacePath::parse(&artifact.path))
            .collect::<Result<Vec<_>, _>>()?;
        let checkpoint = self
            .checkpoint_repository
            .create_checkpoint(
                &dto.run_id,
                "commit_prepare",
                commit_event.seq,
                &required_paths,
            )
            .await?;

        self.event(
            &dto.run_id,
            AgentRunEventLevel::Info,
            "persistent_changes_prepared",
            json!({
                "changeCount": persistent_changes.changes.len(),
                "changes": &persistent_changes.changes,
            }),
        )
        .await?;

        self.event(
            &dto.run_id,
            AgentRunEventLevel::Info,
            "commit_draft_created",
            json!({
                "checkpointId": checkpoint.id.as_str(),
                "artifactPath": file.path.as_str(),
                "bytes": file.bytes,
                "sha256": file.sha256.as_str(),
            }),
        )
        .await?;

        let extra = json!({
            "tauritavern": {
                "agent": {
                    "version": 1,
                    "runId": run.id.as_str(),
                    "workspaceId": run.workspace_id.as_str(),
                    "stableChatId": run.stable_chat_id.as_str(),
                    "profileId": run.profile_id.as_ref(),
                    "checkpointId": checkpoint.id.as_str(),
                    "persistentChanges": &persistent_changes.changes,
                    "artifacts": [{
                        "id": message_artifact.id.as_str(),
                        "path": file.path.as_str(),
                        "kind": message_artifact.kind.as_str(),
                        "target": "message_body",
                        "bytes": file.bytes,
                        "sha256": file.sha256.as_str()
                    }]
                }
            }
        });

        Ok(AgentCommitDraftDto {
            run_id: dto.run_id,
            stable_chat_id: run.stable_chat_id,
            chat_ref: run.chat_ref,
            generation_type: run.generation_type,
            checkpoint,
            message: AgentCommitMessageDto {
                mes: file.text,
                extra: Some(extra),
            },
        })
    }

    pub async fn finalize_commit(
        &self,
        dto: AgentFinalizeCommitDto,
    ) -> Result<AgentCommitResultDto, ApplicationError> {
        let run = self.run_repository.load_run(&dto.run_id).await?;
        if run.status != AgentRunStatus::Committing {
            return Err(ApplicationError::ValidationError(format!(
                "agent.invalid_finalize_state: expected committing, got {:?}",
                run.status
            )));
        }

        let persistent_changes = match self
            .workspace_repository
            .commit_persistent_changes(&dto.run_id)
            .await
        {
            Ok(changes) => changes,
            Err(error) => {
                self.event(
                    &dto.run_id,
                    AgentRunEventLevel::Error,
                    "persistent_changes_commit_failed",
                    json!({ "message": error.to_string() }),
                )
                .await?;
                return Err(error.into());
            }
        };
        self.event(
            &dto.run_id,
            AgentRunEventLevel::Info,
            "persistent_changes_committed",
            json!({
                "changeCount": persistent_changes.changes.len(),
                "changes": &persistent_changes.changes,
            }),
        )
        .await?;

        self.event(
            &dto.run_id,
            AgentRunEventLevel::Info,
            "run_committed",
            json!({ "messageId": dto.message_id }),
        )
        .await?;
        let completed = self
            .transition_status(&dto.run_id, AgentRunStatus::Completed)
            .await?;
        self.event(
            &dto.run_id,
            AgentRunEventLevel::Info,
            "run_completed",
            Value::Null,
        )
        .await?;
        self.active_runs.write().await.remove(&dto.run_id);

        Ok(AgentCommitResultDto {
            run_id: completed.id,
            status: completed.status,
        })
    }
}
