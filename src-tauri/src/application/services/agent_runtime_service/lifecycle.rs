use std::sync::Arc;

use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::watch;
use uuid::Uuid;

use super::AgentRuntimeService;
use super::prompt_snapshot::{
    reject_external_tool_request, request_from_prompt_snapshot,
    validate_prompt_snapshot_context_policy,
};
use super::skill_scope::{
    resolve_run_skill_scope_refs, skill_event_summary, skill_scope_order_for_profile,
};
use super::timeline_projection::build_run_timeline_projection;
use crate::application::dto::agent_dto::{
    AgentCancelRunDto, AgentReadEventsDto, AgentReadEventsResultDto, AgentReadWorkspaceFileDto,
    AgentRunHandleDto, AgentStartRunDto, AgentWorkspaceFileDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_identity::{
    validate_stable_chat_id, workspace_id_for_stable_chat_id,
};
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, ensure_profile_model_configured,
};
use crate::application::services::agent_workspace_lifecycle_service::AgentRunActivity;
use crate::application::services::prompt_assembly_service::attach_frozen_run_input_snapshot;
use crate::domain::models::agent::{AgentRun, AgentRunEventLevel, AgentRunStatus, WorkspacePath};
use crate::domain::repositories::agent_run_repository::AgentRunEventReadQuery;
use crate::domain::text_metrics::TextMetrics;

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
        let Some(prompt_snapshot) = dto.prompt_snapshot.as_ref() else {
            return Err(ApplicationError::ValidationError(
                "agent.prompt_snapshot_required: Agent tool loop requires a concrete prompt snapshot"
                    .to_string(),
            ));
        };
        let request = request_from_prompt_snapshot(prompt_snapshot)?;
        reject_external_tool_request(&request.payload)?;

        let generation_type = dto.generation_type.trim().to_string();
        if generation_type.is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.invalid_generation_type: generationType cannot be empty".to_string(),
            ));
        }

        let mut resolved_profile = self
            .profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id: dto.profile_id.as_deref(),
                known_tools: self.tool_registry.specs(),
            })
            .await?;
        if !resolved_profile.run.direct_runnable {
            return Err(ApplicationError::ValidationError(format!(
                "agent.profile_not_direct_runnable: profile `{}` can only run as a delegated SubAgent",
                resolved_profile.id.as_str()
            )));
        }
        ensure_profile_model_configured(&resolved_profile)?;
        validate_prompt_snapshot_context_policy(prompt_snapshot, &resolved_profile)?;
        let prompt_snapshot = attach_frozen_run_input_snapshot(
            prompt_snapshot.clone(),
            dto.frozen_run_input_snapshot.clone(),
        )?;
        let presentation = dto
            .options
            .presentation
            .unwrap_or(resolved_profile.run.presentation);
        if presentation == crate::domain::models::agent::AgentRunPresentation::Foreground
            && (!resolved_profile
                .tools
                .allow
                .iter()
                .any(|name| name == "workspace.commit")
                || resolved_profile
                    .tools
                    .deny
                    .iter()
                    .any(|name| name == "workspace.commit"))
        {
            return Err(ApplicationError::ValidationError(
                "agent.foreground_commit_unavailable: foreground runs require workspace.commit"
                    .to_string(),
            ));
        }
        resolved_profile.run.presentation = presentation;
        let skill_scope_refs = resolve_run_skill_scope_refs(&dto, &resolved_profile)?;
        let skill_scope_order =
            skill_scope_order_for_profile(&resolved_profile, &skill_scope_refs)?;
        let effective_skills = self
            .skill_service
            .resolve_effective_skills(&skill_scope_order, &resolved_profile.skills)
            .await?;
        let stable_chat_id = validate_stable_chat_id(&dto.stable_chat_id)?;
        let run_id = format!("run_{}", Uuid::new_v4().simple());
        let workspace_id = workspace_id_for_stable_chat_id(&dto.chat_ref, &stable_chat_id)?;
        let input_context = self
            .resolve_agent_run_input_context(&dto.chat_ref, &generation_type)
            .await?;
        if let Some(requested_state_id) = dto.persist_base_state_id.as_deref() {
            let requested_state_id = requested_state_id.trim();
            if requested_state_id.is_empty() {
                return Err(ApplicationError::ValidationError(
                    "agent.persist_base_state_id_empty: persistBaseStateId cannot be empty"
                        .to_string(),
                ));
            }
            if input_context.persist_base_state_id.as_deref() != Some(requested_state_id) {
                return Err(ApplicationError::ValidationError(
                    "agent.persist_base_state_mismatch: requested persistBaseStateId does not match the current chat history"
                        .to_string(),
                ));
            }
        }
        let now = Utc::now();
        let run = AgentRun {
            id: run_id.clone(),
            workspace_id: workspace_id.clone(),
            stable_chat_id: stable_chat_id.clone(),
            chat_ref: dto.chat_ref.clone(),
            generation_type: generation_type.clone(),
            profile_id: Some(resolved_profile.id.as_str().to_string()),
            skill_scope_refs: skill_scope_refs.clone(),
            persist_base_state_id: input_context.persist_base_state_id,
            input_message_count: Some(input_context.input_message_count),
            presentation,
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
                "persistBaseStateId": run.persist_base_state_id.as_deref(),
                "presentation": presentation,
            }),
        )
        .await?;
        self.event(
            &run_id,
            AgentRunEventLevel::Info,
            "profile_resolved",
            json!({
                "profileId": resolved_profile.id.as_str(),
                "source": resolved_profile.source_trace.profile_source.as_str(),
            }),
        )
        .await?;
        self.event(
            &run_id,
            AgentRunEventLevel::Info,
            "skill_scopes_resolved",
            json!({
                "scopes": skill_scope_order,
                "refs": skill_scope_refs,
                "effectiveSkills": skill_event_summary(&effective_skills),
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
        let active_handle = Arc::new(super::scheduler::ActiveRunHandle::new(
            self,
            run_id.clone(),
            cancel_sender,
        ));
        self.active_runs
            .write()
            .await
            .insert(run_id.clone(), active_handle);

        let service = self.clone();
        let background_run_id = run_id.clone();
        tokio::spawn(async move {
            service
                .execute_agent_loop_run(
                    background_run_id,
                    prompt_snapshot,
                    request,
                    resolved_profile,
                    effective_skills,
                    cancel_receiver,
                )
                .await;
        });

        Ok(AgentRunHandleDto {
            run_id,
            workspace_id,
            stable_chat_id,
            generation_type,
            status: AgentRunStatus::Created,
        })
    }

    pub async fn cancel_run(
        &self,
        dto: AgentCancelRunDto,
    ) -> Result<AgentRunHandleDto, ApplicationError> {
        let run = self.run_repository.load_run(&dto.run_id).await?;
        match run.status {
            AgentRunStatus::Completed
            | AgentRunStatus::PartialSuccess
            | AgentRunStatus::Cancelled
            | AgentRunStatus::Failed => {
                return Ok(AgentRunHandleDto {
                    run_id: run.id,
                    workspace_id: run.workspace_id,
                    stable_chat_id: run.stable_chat_id,
                    generation_type: run.generation_type,
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

        let active_handle = self.active_runs.read().await.get(&dto.run_id).cloned();

        let next = if let Some(handle) = active_handle {
            self.close_guidance_mailbox_for_run(
                &dto.run_id,
                "run_cancel_requested",
                AgentRunEventLevel::Info,
            )
            .await?;
            let _ = handle.cancel_sender.send(true);
            handle.scheduler.cancel_all_unfinished().await?;
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
        };

        Ok(AgentRunHandleDto {
            run_id: next.id,
            workspace_id: next.workspace_id,
            stable_chat_id: next.stable_chat_id,
            generation_type: next.generation_type,
            status: next.status,
        })
    }

    pub async fn read_events(
        &self,
        dto: AgentReadEventsDto,
    ) -> Result<AgentReadEventsResultDto, ApplicationError> {
        let invocation_id = normalize_read_events_invocation_id(dto.invocation_id.as_deref())?;
        let events = self
            .run_repository
            .read_events(
                &dto.run_id,
                AgentRunEventReadQuery {
                    after_seq: dto.after_seq,
                    before_seq: dto.before_seq,
                    limit: dto.limit,
                    invocation_id,
                },
            )
            .await?;
        let timeline_projection = if dto.include_timeline_projection {
            let invocations = self
                .invocation_repository
                .list_invocations(&dto.run_id)
                .await?;
            let tasks = self.invocation_repository.list_tasks(&dto.run_id).await?;
            Some(build_run_timeline_projection(&invocations, &tasks)?)
        } else {
            None
        };

        Ok(AgentReadEventsResultDto {
            events,
            timeline_projection,
        })
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

        let metrics = TextMetrics::from_text(&file.text);
        Ok(AgentWorkspaceFileDto {
            path: file.path.as_str().to_string(),
            text: file.text,
            chars: metrics.chars,
            words: metrics.words,
            sha256: file.sha256,
        })
    }
}

fn normalize_read_events_invocation_id(
    value: Option<&str>,
) -> Result<Option<String>, ApplicationError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let invocation_id = value.trim();
    if invocation_id.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.read_events_invocation_id_invalid: invocationId cannot be empty".to_string(),
        ));
    }
    if invocation_id.contains('/') || invocation_id.contains('\\') {
        return Err(ApplicationError::ValidationError(
            "agent.read_events_invocation_id_invalid: invocationId must not contain path separators"
                .to_string(),
        ));
    }
    Ok(Some(invocation_id.to_string()))
}

#[async_trait::async_trait]
impl AgentRunActivity for AgentRuntimeService {
    async fn active_run_ids(&self) -> Result<Vec<String>, ApplicationError> {
        let mut run_ids = self
            .active_runs
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        run_ids.sort();
        Ok(run_ids)
    }

    async fn active_run_ids_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<String>, ApplicationError> {
        let run_ids = self.active_run_ids().await?;
        let mut active = Vec::new();
        for run_id in run_ids {
            let run = self.run_repository.load_run(&run_id).await?;
            if run.workspace_id == workspace_id {
                active.push(run_id);
            }
        }
        active.sort();
        Ok(active)
    }
}
