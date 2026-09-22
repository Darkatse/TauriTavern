use serde_json::{Value, json};
use tokio::sync::oneshot;
use tt_ports::workspace_fs::WorkspaceWriteGuard;
use uuid::Uuid;

use super::{
    AgentCancelReceiver, AgentRuntimeService, HostPromptAssemblyResult, PendingHostPromptAssembly,
};
use crate::dto::agent_dto::{
    AgentPreparePromptAssemblyDto, AgentPromptAssemblyBrokerRequestDto, AgentPromptAssemblyModeDto,
    AgentPromptAssemblyScopeDto, AgentReadPromptAssemblyRequestDto, AgentResolvePromptAssemblyDto,
};
use crate::errors::ApplicationError;
use crate::services::prompt_assembly_service::AgentInvocationPromptAssemblyContext;
use tt_domain::models::agent::profile::{AgentPresetBindingMode, ResolvedAgentProfile};
use tt_domain::models::agent::{AgentModelTool, AgentRunEventLevel, WorkspacePath};

impl AgentRuntimeService {
    pub async fn prepare_session_run(
        &self,
        dto: crate::dto::agent_dto::AgentPrepareSessionRunDto,
    ) -> Result<crate::dto::agent_dto::AgentPrepareSessionRunResultDto, ApplicationError> {
        use tt_domain::models::agent::{
            AgentInvocationExitPolicy, AgentModelContentPart, AgentModelMessage, AgentModelRole,
        };
        use tt_ports::repositories::agent_session_repository::AgentSessionMessageReadQuery;

        self.session_repository
            .load_session(&dto.session_id)
            .await?;
        if dto.text.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.session_text_required: text must not be empty".into(),
            ));
        }
        let profile = self
            .profile_service
            .resolve_session_profile(dto.profile, self.tool_catalog())
            .await?;
        // Assembly needs the full transcript; read it once, then let PromptManager budget it.
        let history = self
            .session_repository
            .read_session_messages(
                &dto.session_id,
                AgentSessionMessageReadQuery {
                    after_seq: Some(0),
                    before_seq: None,
                    limit: usize::MAX,
                },
            )
            .await?;
        let last_seq = history.last().map_or(0, |entry| entry.seq);
        let mut messages = history
            .into_iter()
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        messages.push(AgentModelMessage {
            role: AgentModelRole::User,
            parts: vec![AgentModelContentPart::Text { text: dto.text }],
            provider_metadata: Value::Null,
        });
        let tools = self
            .prepare_invocation_tools(
                &profile,
                tt_domain::models::tool::AgentToolScope::Session,
                AgentInvocationExitPolicy::ReplyAllowed,
                "session_prepare",
            )
            .await?;
        let assembly = self
            .prompt_assembly_service
            .prepare_frontend_prompt_assembly(
                AgentPreparePromptAssemblyDto {
                    profile_id: None,
                    generation_type: "normal".into(),
                    frozen_run_input_snapshot: json!({
                        "schemaVersion": 1,
                        "kind": "tauritavern.agentFrozenRunInputSnapshot",
                        "contextKind": "session",
                        "generationType": "normal",
                        "promptInputs": { "messages": [], "agentMessages": messages },
                        "worldInfoActivation": {},
                        "macroContext": {},
                    }),
                    json_schema: None,
                },
                profile,
                &tools.model_tools,
                AgentInvocationExitPolicy::ReplyAllowed,
            )
            .await?;
        Ok(crate::dto::agent_dto::AgentPrepareSessionRunResultDto {
            expected_history_seq: last_seq,
            assembly,
        })
    }

    pub async fn read_prompt_assembly_request(
        &self,
        dto: AgentReadPromptAssemblyRequestDto,
    ) -> Result<AgentPromptAssemblyBrokerRequestDto, ApplicationError> {
        let run_id = dto.run_id.trim();
        let assembly_id = dto.assembly_id.trim();
        if run_id.is_empty() || assembly_id.is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.prompt_assembly_request_read_invalid: runId and assemblyId are required"
                    .to_string(),
            ));
        }

        let assemblies = self.active_prompt_assemblies.read().await;
        let pending = assemblies
            .get(assembly_id)
            .ok_or_else(|| prompt_assembly_not_pending_error(assembly_id))?;
        if pending.run_id != run_id {
            return Err(prompt_assembly_run_mismatch_error(assembly_id));
        }

        Ok(pending.request.clone())
    }

    pub async fn resolve_prompt_assembly(
        &self,
        dto: AgentResolvePromptAssemblyDto,
    ) -> Result<(), ApplicationError> {
        let run_id = dto.run_id.trim();
        let assembly_id = dto.assembly_id.trim();
        if run_id.is_empty() || assembly_id.is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.prompt_assembly_resolve_invalid: runId and assemblyId are required"
                    .to_string(),
            ));
        }

        {
            let assemblies = self.active_prompt_assemblies.read().await;
            let pending = assemblies
                .get(assembly_id)
                .ok_or_else(|| prompt_assembly_not_pending_error(assembly_id))?;
            if pending.run_id != run_id {
                return Err(prompt_assembly_run_mismatch_error(assembly_id));
            }
        }

        let result = match dto.error.map(|value| value.trim().to_string()) {
            Some(error) if !error.is_empty() => Err(error),
            _ => {
                let prompt_snapshot = dto.prompt_snapshot.ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "agent.prompt_assembly_snapshot_required: promptSnapshot is required"
                            .to_string(),
                    )
                })?;
                Ok(HostPromptAssemblyResult {
                    prompt_snapshot,
                    frozen_run_input_snapshot: dto.frozen_run_input_snapshot,
                    generation_intent: dto.generation_intent,
                    assembly: dto.assembly,
                })
            }
        };

        let pending = {
            let mut assemblies = self.active_prompt_assemblies.write().await;
            let pending = assemblies
                .remove(assembly_id)
                .ok_or_else(|| prompt_assembly_not_pending_error(assembly_id))?;
            if pending.run_id != run_id {
                assemblies.insert(assembly_id.to_string(), pending);
                return Err(prompt_assembly_run_mismatch_error(assembly_id));
            }
            pending
        };

        pending.sender.send(result).map_err(|_| {
            ApplicationError::ValidationError(format!(
                "agent.prompt_assembly_resolve_failed: run `{run_id}` is no longer waiting for assembly `{assembly_id}`"
            ))
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "prompt assembly boundary keeps profile, tools, snapshot, scope, and cancellation explicit"
    )]
    pub(super) async fn assemble_invocation_prompt_snapshot(
        &self,
        profile: &ResolvedAgentProfile,
        visible_tools: &[AgentModelTool],
        generation_type: &str,
        frozen_run_input_snapshot: Value,
        scope: &AgentPromptAssemblyScopeDto,
        agent_task_prompt: String,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<Option<Value>, ApplicationError> {
        let run_id = scope.run_id.as_str();
        let invocation_id = scope.invocation_id.as_str();
        if profile.preset.mode != AgentPresetBindingMode::Ref {
            return Ok(None);
        }
        let assembly_id = format!("prompt_assembly_{}", Uuid::new_v4().simple());
        let prepared = self
            .prompt_assembly_service
            .prepare_invocation_frontend_prompt_assembly(
                AgentPreparePromptAssemblyDto {
                    profile_id: Some(profile.id.as_str().to_string()),
                    generation_type: generation_type.to_string(),
                    frozen_run_input_snapshot,
                    json_schema: None,
                },
                profile.clone(),
                visible_tools,
                AgentInvocationPromptAssemblyContext {
                    assembly_id: assembly_id.clone(),
                    scope: scope.clone(),
                    agent_task_prompt: Some(agent_task_prompt),
                    required_agent_prompt_components: vec![
                        "agentSystemPrompt".to_string(),
                        "agentTask".to_string(),
                    ],
                },
            )
            .await?;
        if matches!(
            prepared.mode,
            AgentPromptAssemblyModeDto::CurrentPromptSnapshot
        ) {
            return Ok(None);
        }
        let request = prepared.request.ok_or_else(|| {
            ApplicationError::InternalError(
                "agent.prompt_assembly_request_missing: frontend prompt assembly mode requires request"
                    .to_string(),
            )
        })?;
        let request_metadata = prepared.assembly.ok_or_else(|| {
            ApplicationError::InternalError(
                "agent.prompt_assembly_metadata_missing: frontend prompt assembly mode requires metadata"
                    .to_string(),
            )
        })?;
        let requested_event = json!({
            "assemblyId": assembly_id.as_str(),
            "invocationId": invocation_id,
            "profileId": profile.id.as_str(),
            "scope": &scope,
            "requestKind": request.kind.as_str(),
            "requestSchemaVersion": request.schema_version,
            "requestFingerprint": &request.fingerprint,
        });

        let (sender, receiver) = oneshot::channel();
        let previous = self.active_prompt_assemblies.write().await.insert(
            assembly_id.clone(),
            PendingHostPromptAssembly {
                run_id: run_id.to_string(),
                request,
                sender,
            },
        );
        if previous.is_some() {
            return Err(ApplicationError::InternalError(format!(
                "agent.prompt_assembly_id_collision: duplicate assembly id `{assembly_id}`"
            )));
        }

        if let Err(error) = self
            .event(
                run_id,
                AgentRunEventLevel::Info,
                "prompt_assembly_requested",
                requested_event,
            )
            .await
        {
            self.active_prompt_assemblies
                .write()
                .await
                .remove(&assembly_id);
            return Err(error);
        }

        let host_result = tokio::select! {
            result = receiver => {
                result.map_err(|_| ApplicationError::InternalError(format!(
                    "agent.prompt_assembly_channel_closed: host assembly `{assembly_id}` closed before resolution"
                )))?
            }
            changed = cancel.changed() => {
                let _ = changed;
                self.active_prompt_assemblies.write().await.remove(&assembly_id);
                self.ensure_not_cancelled(cancel)?;
                return Err(ApplicationError::Cancelled(
                    "Agent run cancelled while awaiting host prompt assembly".to_string(),
                ));
            }
        };

        match host_result {
            Ok(result) => {
                if let Err(error) = super::prompt_snapshot::validate_prompt_snapshot_context_policy(
                    &result.prompt_snapshot,
                    profile,
                ) {
                    self.event(
                        run_id,
                        AgentRunEventLevel::Error,
                        "prompt_assembly_failed",
                        json!({
                            "assemblyId": assembly_id.as_str(),
                            "invocationId": invocation_id,
                            "profileId": profile.id.as_str(),
                            "message": error.to_string(),
                        }),
                    )
                    .await?;
                    return Err(error);
                }
                let snapshot_path = WorkspacePath::parse(format!(
                    "input/invocations/{invocation_id}/prompt_snapshot.json"
                ))?;
                let assembly_path = WorkspacePath::parse(format!(
                    "input/invocations/{invocation_id}/prompt_assembly.json"
                ))?;
                self.workspace_files(run_id)
                    .await?
                    .write_text(
                        &snapshot_path,
                        &serde_json::to_string_pretty(&result.prompt_snapshot).map_err(
                            |error| {
                                ApplicationError::InternalError(format!(
                                    "agent.prompt_assembly_snapshot_serialize_failed: {error}"
                                ))
                            },
                        )?,
                        WorkspaceWriteGuard::Unchecked,
                    )
                    .await?;
                self.workspace_files(run_id)
                    .await?
                    .write_text(
                        &assembly_path,
                        &serde_json::to_string_pretty(&json!({
                            "assemblyId": assembly_id.as_str(),
                            "invocationId": invocation_id,
                            "profileId": profile.id.as_str(),
                            "scope": scope,
                            "requestMetadata": request_metadata,
                            "frozenRunInputSnapshot": result.frozen_run_input_snapshot,
                            "generationIntent": result.generation_intent,
                            "assembly": result.assembly,
                        }))
                        .map_err(|error| {
                            ApplicationError::InternalError(format!(
                                "agent.prompt_assembly_metadata_serialize_failed: {error}"
                            ))
                        })?,
                        WorkspaceWriteGuard::Unchecked,
                    )
                    .await?;
                self.event(
                    run_id,
                    AgentRunEventLevel::Info,
                    "prompt_assembly_completed",
                    json!({
                        "assemblyId": assembly_id.as_str(),
                        "invocationId": invocation_id,
                        "profileId": profile.id.as_str(),
                        "promptSnapshotPath": snapshot_path.as_str(),
                        "promptAssemblyPath": assembly_path.as_str(),
                    }),
                )
                .await?;
                Ok(Some(result.prompt_snapshot))
            }
            Err(message) => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "prompt_assembly_failed",
                    json!({
                        "assemblyId": assembly_id.as_str(),
                        "invocationId": invocation_id,
                        "profileId": profile.id.as_str(),
                        "message": message,
                    }),
                )
                .await?;
                Err(ApplicationError::ValidationError(format!(
                    "agent.prompt_assembly_failed: {message}"
                )))
            }
        }
    }
}

fn prompt_assembly_not_pending_error(assembly_id: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.prompt_assembly_not_pending: assembly `{assembly_id}` is not awaiting host resolution"
    ))
}

fn prompt_assembly_run_mismatch_error(assembly_id: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "agent.prompt_assembly_run_mismatch: assembly `{assembly_id}` belongs to another run"
    ))
}
