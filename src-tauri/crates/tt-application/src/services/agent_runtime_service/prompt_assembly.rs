use serde_json::{Value, json};
use tokio::sync::oneshot;
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
use tt_domain::models::agent::{AgentRunEventLevel, AgentToolSpec, WorkspacePath};

impl AgentRuntimeService {
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

    pub(super) async fn assemble_invocation_prompt_snapshot(
        &self,
        run_id: &str,
        invocation_id: &str,
        profile: &ResolvedAgentProfile,
        visible_tools: &[AgentToolSpec],
        generation_type: &str,
        frozen_run_input_snapshot: Value,
        scope: AgentPromptAssemblyScopeDto,
        agent_task_prompt: String,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<Option<Value>, ApplicationError> {
        if profile.preset.mode != AgentPresetBindingMode::Ref {
            return Ok(None);
        }
        let prompt_assembly_service = self.prompt_assembly_service.as_ref().ok_or_else(|| {
            ApplicationError::InternalError(
                "agent.prompt_assembly_service_unavailable: AgentRuntimeService was not configured with PromptAssemblyService"
                    .to_string(),
            )
        })?;
        let assembly_id = format!("prompt_assembly_{}", Uuid::new_v4().simple());
        let prepared = prompt_assembly_service
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

        let (sender, receiver) = oneshot::channel();
        let previous = self.active_prompt_assemblies.write().await.insert(
            assembly_id.clone(),
            PendingHostPromptAssembly {
                run_id: run_id.to_string(),
                request: request.clone(),
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
                json!({
                    "assemblyId": assembly_id.as_str(),
                    "invocationId": invocation_id,
                    "profileId": profile.id.as_str(),
                    "scope": &scope,
                    "requestKind": request.kind.as_str(),
                    "requestSchemaVersion": request.schema_version,
                    "requestFingerprint": &request.fingerprint,
                }),
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
                self.workspace_repository
                    .write_text(
                        run_id,
                        &snapshot_path,
                        &serde_json::to_string_pretty(&result.prompt_snapshot).map_err(
                            |error| {
                                ApplicationError::InternalError(format!(
                                    "agent.prompt_assembly_snapshot_serialize_failed: {error}"
                                ))
                            },
                        )?,
                    )
                    .await?;
                self.workspace_repository
                    .write_text(
                        run_id,
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
