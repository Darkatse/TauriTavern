use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, watch};
use uuid::Uuid;

use crate::application::dto::agent_dto::{
    AgentCancelRunDto, AgentCommitDraftDto, AgentCommitMessageDto, AgentCommitResultDto,
    AgentFinalizeCommitDto, AgentPrepareCommitDto, AgentReadEventsDto, AgentReadEventsResultDto,
    AgentReadWorkspaceFileDto, AgentRunHandleDto, AgentStartRunDto, AgentWorkspaceFileDto,
};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentRun, AgentRunEventLevel, AgentRunStatus, ArtifactSpec, ArtifactTarget, CommitPolicy,
    WorkspaceInputManifest, WorkspaceManifest, WorkspacePath,
};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

type AgentCancelReceiver = watch::Receiver<bool>;

pub struct AgentRuntimeService {
    run_repository: Arc<dyn AgentRunRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    checkpoint_repository: Arc<dyn CheckpointRepository>,
    chat_completion_service: Arc<ChatCompletionService>,
    active_runs: RwLock<HashMap<String, watch::Sender<bool>>>,
}

impl AgentRuntimeService {
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        checkpoint_repository: Arc<dyn CheckpointRepository>,
        chat_completion_service: Arc<ChatCompletionService>,
    ) -> Self {
        Self {
            run_repository,
            workspace_repository,
            checkpoint_repository,
            chat_completion_service,
            active_runs: RwLock::new(HashMap::new()),
        }
    }

    pub async fn start_run(
        self: &Arc<Self>,
        dto: AgentStartRunDto,
    ) -> Result<AgentRunHandleDto, ApplicationError> {
        if dto.options.stream {
            return Err(ApplicationError::ValidationError(
                "agent.phase1_stream_unsupported: Agent Phase 1 only supports non-streaming model calls"
                    .to_string(),
            ));
        }
        if dto.options.auto_commit {
            return Err(ApplicationError::ValidationError(
                "agent.phase1_auto_commit_unsupported: commit is owned by the frontend adapter in Agent Phase 1"
                    .to_string(),
            ));
        }

        let Some(prompt_snapshot) = dto.prompt_snapshot else {
            return Err(ApplicationError::ValidationError(
                "agent.prompt_snapshot_required: Phase 1 requires a concrete prompt snapshot"
                    .to_string(),
            ));
        };
        let request = request_from_prompt_snapshot(&prompt_snapshot)?;
        reject_phase1_tool_request(&request.payload)?;

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
                .execute_one_step_run(background_run_id, prompt_snapshot, request, cancel_receiver)
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

    pub async fn prepare_commit(
        &self,
        dto: AgentPrepareCommitDto,
    ) -> Result<AgentCommitDraftDto, ApplicationError> {
        let run = self.run_repository.load_run(&dto.run_id).await?;
        let run = match run.status {
            AgentRunStatus::AwaitingCommit => {
                self.transition_status(&dto.run_id, AgentRunStatus::Committing)
                    .await?
            }
            AgentRunStatus::Committing => run,
            status => {
                return Err(ApplicationError::ValidationError(format!(
                    "agent.invalid_commit_state: expected awaiting_commit or committing, got {:?}",
                    status
                )));
            }
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
            "commit_draft_created",
            json!({
                "checkpointId": checkpoint.id,
                "artifactPath": file.path.as_str(),
                "bytes": file.bytes,
                "sha256": file.sha256,
            }),
        )
        .await?;

        let extra = json!({
            "tauritavern": {
                "agent": {
                    "version": 1,
                    "runId": run.id,
                    "workspaceId": run.workspace_id,
                    "stableChatId": run.stable_chat_id.clone(),
                    "profileId": run.profile_id,
                    "checkpointId": checkpoint.id,
                    "artifacts": [{
                        "id": message_artifact.id,
                        "path": file.path.as_str(),
                        "kind": message_artifact.kind,
                        "target": "message_body",
                        "bytes": file.bytes,
                        "sha256": file.sha256
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

    async fn execute_one_step_run(
        self: Arc<Self>,
        run_id: String,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        mut cancel: AgentCancelReceiver,
    ) {
        let result = self
            .execute_one_step_run_inner(&run_id, prompt_snapshot, request, &mut cancel)
            .await;

        match result {
            Ok(()) => {}
            Err(ApplicationError::Cancelled(message)) => {
                let _ = self
                    .transition_status(&run_id, AgentRunStatus::Cancelled)
                    .await;
                let _ = self
                    .event(
                        &run_id,
                        AgentRunEventLevel::Info,
                        "run_cancelled",
                        json!({ "message": message }),
                    )
                    .await;
                self.active_runs.write().await.remove(&run_id);
            }
            Err(error) => {
                let _ = self
                    .transition_status(&run_id, AgentRunStatus::Failed)
                    .await;
                let _ = self
                    .event(
                        &run_id,
                        AgentRunEventLevel::Error,
                        "run_failed",
                        json!({ "message": error.to_string() }),
                    )
                    .await;
                self.active_runs.write().await.remove(&run_id);
            }
        }
    }

    async fn execute_one_step_run_inner(
        &self,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let run = self
            .transition_status(run_id, AgentRunStatus::InitializingWorkspace)
            .await?;
        let manifest = build_phase1_manifest(&run);
        self.workspace_repository
            .initialize_run(&run, &manifest, &prompt_snapshot)
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
        self.ensure_not_cancelled(cancel)?;

        self.transition_status(run_id, AgentRunStatus::AssemblingContext)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "context_assembled",
            request_summary(&request.payload),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        self.transition_status(run_id, AgentRunStatus::CallingModel)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "model_request_created",
            request_summary(&request.payload),
        )
        .await?;

        let response = self
            .chat_completion_service
            .generate_with_cancel(request, cancel.clone())
            .await?;
        reject_phase1_tool_response(&response)?;
        let text = extract_response_text(&response)?;
        if text.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "model.empty_response_text: model response text is empty".to_string(),
            ));
        }
        self.ensure_not_cancelled(cancel)?;

        self.transition_status(run_id, AgentRunStatus::ApplyingWorkspacePatch)
            .await?;
        let artifact_path = WorkspacePath::parse("output/main.md")?;
        let artifact = self
            .workspace_repository
            .write_text(run_id, &artifact_path, &text)
            .await?;
        let artifact_event = self
            .event(
                run_id,
                AgentRunEventLevel::Info,
                "workspace_file_written",
                json!({
                    "path": artifact.path.as_str(),
                    "bytes": artifact.bytes,
                    "sha256": artifact.sha256,
                }),
            )
            .await?;
        self.ensure_not_cancelled(cancel)?;

        self.transition_status(run_id, AgentRunStatus::CreatingCheckpoint)
            .await?;
        let checkpoint = self
            .checkpoint_repository
            .create_checkpoint(
                run_id,
                "model_response",
                artifact_event.seq,
                &[artifact_path],
            )
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "checkpoint_created",
            json!({ "checkpointId": checkpoint.id }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        self.transition_status(run_id, AgentRunStatus::AssemblingArtifacts)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "artifact_assembled",
            json!({
                "id": "main",
                "path": artifact.path.as_str(),
                "checkpointId": checkpoint.id,
            }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        self.transition_status(run_id, AgentRunStatus::AwaitingCommit)
            .await?;
        Ok(())
    }

    async fn transition_status(
        &self,
        run_id: &str,
        status: AgentRunStatus,
    ) -> Result<AgentRun, ApplicationError> {
        let mut run = self.run_repository.load_run(run_id).await?;
        run.status = status;
        run.updated_at = Utc::now();
        self.run_repository.save_run(&run).await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "status_changed",
            json!({ "status": status }),
        )
        .await?;
        Ok(run)
    }

    async fn event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<crate::domain::models::agent::AgentRunEvent, ApplicationError> {
        self.run_repository
            .append_event(run_id, level, event_type, payload)
            .await
            .map_err(ApplicationError::from)
    }

    fn ensure_not_cancelled(&self, cancel: &AgentCancelReceiver) -> Result<(), ApplicationError> {
        if *cancel.borrow() {
            return Err(DomainError::generation_cancelled_by_user().into());
        }
        Ok(())
    }
}

fn build_phase1_manifest(run: &AgentRun) -> WorkspaceManifest {
    WorkspaceManifest {
        workspace_version: 1,
        run_id: run.id.clone(),
        stable_chat_id: run.stable_chat_id.clone(),
        chat_ref: run.chat_ref.clone(),
        created_at: Utc::now(),
        input: WorkspaceInputManifest {
            mode: "prompt_snapshot".to_string(),
            prompt_snapshot_path: "input/prompt_snapshot.json".to_string(),
        },
        artifacts: vec![ArtifactSpec {
            id: "main".to_string(),
            path: "output/main.md".to_string(),
            kind: "markdown".to_string(),
            target: ArtifactTarget::MessageBody,
            required: true,
            assembly_order: 0,
        }],
        commit_policy: CommitPolicy {
            default_target: ArtifactTarget::MessageBody,
            combine_template: None,
            store_artifacts_in_extra: true,
        },
    }
}

fn workspace_id_for_stable_chat_id(
    chat_ref: &crate::domain::models::agent::AgentChatRef,
    stable_chat_id: &str,
) -> Result<String, ApplicationError> {
    let kind = match chat_ref {
        crate::domain::models::agent::AgentChatRef::Character { .. } => "character",
        crate::domain::models::agent::AgentChatRef::Group { .. } => "group",
    };
    let json = serde_json::to_vec(&json!({
        "kind": kind,
        "stableChatId": stable_chat_id,
    }))
    .map_err(|error| {
        ApplicationError::ValidationError(format!("agent.invalid_chat_ref: {error}"))
    })?;
    let digest = Sha256::digest(json);
    let mut suffix = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        suffix.push_str(&format!("{byte:02x}"));
    }
    Ok(format!("chat_{suffix}"))
}

fn validate_stable_chat_id(raw: &str) -> Result<String, ApplicationError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.stable_chat_id_required: stableChatId is required".to_string(),
        ));
    }
    if value.len() > 512 {
        return Err(ApplicationError::ValidationError(
            "agent.invalid_stable_chat_id: stableChatId is too long".to_string(),
        ));
    }
    Ok(value.to_string())
}

fn request_from_prompt_snapshot(
    prompt_snapshot: &Value,
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let payload = find_payload_object(prompt_snapshot).ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: expected a chat completion payload object".to_string(),
        )
    })?;
    let mut payload = payload.clone();

    payload.insert("stream".to_string(), Value::Bool(false));
    if !payload.contains_key("chat_completion_source") {
        payload.insert(
            "chat_completion_source".to_string(),
            Value::String("openai".to_string()),
        );
    }

    if !payload.contains_key("messages") && !payload.contains_key("prompt") {
        return Err(ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: payload must contain messages or prompt".to_string(),
        ));
    }

    Ok(ChatCompletionGenerateRequestDto { payload })
}

fn find_payload_object(value: &Value) -> Option<Map<String, Value>> {
    let object = value.as_object()?;

    for key in [
        "chatCompletionPayload",
        "chat_completion_payload",
        "generateData",
        "generate_data",
    ] {
        if let Some(payload) = object.get(key).and_then(Value::as_object) {
            return Some(payload.clone());
        }
    }

    if object.contains_key("messages") || object.contains_key("prompt") {
        return Some(object.clone());
    }

    None
}

fn reject_phase1_tool_request(payload: &Map<String, Value>) -> Result<(), ApplicationError> {
    let has_tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    if has_tools {
        return Err(ApplicationError::ValidationError(
            "model.tool_call_unsupported_phase1: Agent Phase 1 does not execute model tool calls"
                .to_string(),
        ));
    }
    Ok(())
}

fn reject_phase1_tool_response(response: &Value) -> Result<(), ApplicationError> {
    let choices = response
        .get("choices")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    for choice in choices {
        if choice.get("finish_reason").and_then(Value::as_str) == Some("tool_calls") {
            return Err(ApplicationError::ValidationError(
                "model.tool_call_unsupported_phase1: model returned tool_calls finish reason"
                    .to_string(),
            ));
        }
        if choice
            .pointer("/message/tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|tool_calls| !tool_calls.is_empty())
        {
            return Err(ApplicationError::ValidationError(
                "model.tool_call_unsupported_phase1: model returned tool calls".to_string(),
            ));
        }
    }

    if response
        .pointer("/message/tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|tool_calls| !tool_calls.is_empty())
    {
        return Err(ApplicationError::ValidationError(
            "model.tool_call_unsupported_phase1: model returned tool calls".to_string(),
        ));
    }

    Ok(())
}

fn extract_response_text(response: &Value) -> Result<String, ApplicationError> {
    if let Some(text) = response.as_str() {
        return Ok(text.to_string());
    }

    for pointer in [
        "/choices/0/message/content",
        "/choices/0/text",
        "/text",
        "/message/content",
        "/message/tool_plan",
        "/output",
        "/content",
    ] {
        if let Some(text) = text_from_value(response.pointer(pointer)) {
            return Ok(text);
        }
    }

    Err(ApplicationError::ValidationError(
        "model.empty_response_text: could not extract assistant message text from model response"
            .to_string(),
    ))
}

fn text_from_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut output = String::new();
            for part in parts {
                match part {
                    Value::String(text) => output.push_str(text),
                    Value::Object(object) => {
                        if object.get("type").and_then(Value::as_str) == Some("tool_use") {
                            return None;
                        }
                        if let Some(text) = object.get("text").and_then(Value::as_str) {
                            output.push_str(text);
                        } else if let Some(text) = object.get("content").and_then(Value::as_str) {
                            output.push_str(text);
                        }
                    }
                    _ => {}
                }
            }
            Some(output)
        }
        _ => None,
    }
}

fn request_summary(payload: &Map<String, Value>) -> Value {
    json!({
        "chatCompletionSource": payload.get("chat_completion_source").and_then(Value::as_str),
        "model": payload.get("model").and_then(Value::as_str),
        "messageCount": payload.get("messages").and_then(Value::as_array).map(|messages| messages.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_message_content() {
        let response = json!({
            "choices": [{
                "message": { "content": "hello" }
            }]
        });

        assert_eq!(extract_response_text(&response).unwrap(), "hello");
    }

    #[test]
    fn rejects_tool_call_response() {
        let response = json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": { "tool_calls": [{ "id": "call_1" }] }
            }]
        });

        assert!(reject_phase1_tool_response(&response).is_err());
    }

    #[test]
    fn workspace_id_uses_stable_chat_id_not_character_chat_file_name() {
        let first = crate::domain::models::agent::AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "old-chat".to_string(),
        };
        let second = crate::domain::models::agent::AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "renamed-chat".to_string(),
        };

        let first_id = workspace_id_for_stable_chat_id(&first, "stable-chat").unwrap();
        let second_id = workspace_id_for_stable_chat_id(&second, "stable-chat").unwrap();

        assert_eq!(first_id, second_id);
    }
}
