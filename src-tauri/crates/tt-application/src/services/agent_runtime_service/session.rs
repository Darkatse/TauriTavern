use std::sync::Arc;

use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::watch;
use uuid::Uuid;

use super::prompt_snapshot::{
    reject_external_tool_request, request_from_prompt_snapshot,
    validate_prompt_snapshot_context_policy,
};
use super::{ActiveRunHandle, AgentRuntimeService};
use crate::dto::agent_dto::{
    AgentCreateSessionResultDto, AgentReadSessionDto, AgentReadSessionResultDto,
    AgentSaveProfileDto, AgentSessionProfileResultDto, AgentSessionRunHandleDto,
    AgentStartSessionRunDto,
};
use crate::errors::ApplicationError;
use crate::services::prompt_assembly_service::attach_frozen_run_input_snapshot;
use tt_domain::models::agent::session::AgentSession;
use tt_domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRole, AgentRun, AgentRunEventLevel,
    AgentRunStatus, AgentRunTarget,
};
use tt_ports::repositories::agent_session_repository::AgentSessionMessageReadQuery;

impl AgentRuntimeService {
    pub async fn load_session_profile(
        &self,
    ) -> Result<AgentSessionProfileResultDto, ApplicationError> {
        Ok(AgentSessionProfileResultDto {
            profile: self.session_repository.load_session_profile().await?,
        })
    }

    pub async fn save_session_profile(
        &self,
        dto: AgentSaveProfileDto,
    ) -> Result<(), ApplicationError> {
        self.profile_service
            .resolve_session_profile(dto.profile.clone(), self.tool_registry.catalog())
            .await?;
        self.session_repository
            .save_session_profile(&dto.profile)
            .await?;
        Ok(())
    }

    pub async fn create_session(&self) -> Result<AgentCreateSessionResultDto, ApplicationError> {
        let session = AgentSession {
            id: format!("session_{}", Uuid::new_v4().simple()),
            created_at: Utc::now(),
        };
        self.session_repository.create_session(&session).await?;
        Ok(AgentCreateSessionResultDto { session })
    }

    pub async fn read_session(
        &self,
        dto: AgentReadSessionDto,
    ) -> Result<AgentReadSessionResultDto, ApplicationError> {
        let limit = dto.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err(ApplicationError::ValidationError(
                "agent.session_page_limit_invalid: limit must be between 1 and 200".into(),
            ));
        }
        let session = self
            .session_repository
            .load_session(&dto.session_id)
            .await?;
        let mut messages = self
            .session_repository
            .read_session_messages(
                &session.id,
                AgentSessionMessageReadQuery {
                    after_seq: None,
                    before_seq: dto.before_seq,
                    limit: limit + 1,
                },
            )
            .await?;
        let next_before_seq = if messages.len() > limit {
            messages.remove(0);
            messages.first().map(|message| message.seq)
        } else {
            None
        };
        let last_seq = self
            .session_repository
            .session_last_seq(&session.id)
            .await?;
        let active_id = self
            .active_runs
            .read()
            .await
            .iter()
            .find(|(_, handle)| handle.target.session_id() == Some(session.id.as_str()))
            .map(|(run_id, _)| run_id.clone());
        let active_run = if let Some(run_id) = active_id {
            let run = self.run_repository.load_run(&run_id).await?;
            Some(AgentSessionRunHandleDto {
                session_id: session.id.clone(),
                run_id,
                status: run.status,
            })
        } else {
            None
        };
        Ok(AgentReadSessionResultDto {
            session,
            messages,
            last_seq,
            next_before_seq,
            active_run,
        })
    }

    pub async fn start_session_run(
        self: &Arc<Self>,
        dto: AgentStartSessionRunDto,
    ) -> Result<AgentSessionRunHandleDto, ApplicationError> {
        if dto.text.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.session_text_required: text cannot be empty".into(),
            ));
        }
        let profile = self
            .profile_service
            .resolve_session_profile(dto.profile, self.tool_registry.catalog())
            .await?;
        let effective_skills = self.resolve_session_skills(&profile).await?;
        let request = request_from_prompt_snapshot(&dto.prompt_snapshot)?;
        reject_external_tool_request(&request.payload)?;
        validate_prompt_snapshot_context_policy(&dto.prompt_snapshot, &profile)?;
        let prompt_snapshot = attach_frozen_run_input_snapshot(
            dto.prompt_snapshot,
            Some(dto.frozen_run_input_snapshot),
        )?;
        let _admission = self.run_lifecycle_lock.lock().await;
        self.session_repository
            .load_session(&dto.session_id)
            .await?;
        if self
            .active_runs
            .read()
            .await
            .values()
            .any(|handle| handle.target.session_id() == Some(dto.session_id.as_str()))
        {
            return Err(ApplicationError::ValidationError(
                "agent.session_busy: this Session is already running".into(),
            ));
        }
        if self
            .session_repository
            .session_last_seq(&dto.session_id)
            .await?
            != dto.expected_history_seq
        {
            return Err(ApplicationError::ValidationError("agent.session_history_changed: prepare the prompt again using the current Session history".into()));
        }
        let now = Utc::now();
        let run = AgentRun {
            id: format!("run_{}", Uuid::new_v4().simple()),
            workspace_id: dto.session_id.clone(),
            target: AgentRunTarget::Session {
                session_id: dto.session_id.clone(),
            },
            profile_id: Some(profile.id.as_str().to_string()),
            status: AgentRunStatus::Created,
            created_at: now,
            updated_at: now,
        };
        self.run_repository.create_run(&run).await?;
        let user_message = self
            .session_repository
            .append_session_message(
                &dto.session_id,
                &run.id,
                &AgentModelMessage {
                    role: AgentModelRole::User,
                    parts: vec![AgentModelContentPart::Text { text: dto.text }],
                    provider_metadata: Value::Null,
                },
            )
            .await?;
        self.event(
            &run.id,
            AgentRunEventLevel::Info,
            "run_created",
            json!({
                "workspaceId": run.workspace_id,
                "target": run.target,
            }),
        )
        .await?;
        self.event(
            &run.id,
            AgentRunEventLevel::Info,
            "session_message_appended",
            json!({
                "sessionId": dto.session_id, "seq": user_message.seq,
            }),
        )
        .await?;
        if let Some(intent) = dto.generation_intent {
            self.event(
                &run.id,
                AgentRunEventLevel::Info,
                "generation_intent_recorded",
                intent,
            )
            .await?;
        }
        let (cancel_sender, cancel_receiver) = watch::channel(false);
        let handle = Arc::new(ActiveRunHandle::new(
            self,
            &run,
            self.workspace_repository.open_filesystem(&run.id).await?,
            cancel_sender,
            dto.stream,
            false,
        ));
        self.active_runs
            .write()
            .await
            .insert(run.id.clone(), handle);
        let service = self.clone();
        let run_id = run.id.clone();
        tokio::spawn(async move {
            service
                .execute_agent_loop_run(
                    run_id,
                    prompt_snapshot,
                    request,
                    profile,
                    effective_skills,
                    cancel_receiver,
                )
                .await;
        });
        Ok(AgentSessionRunHandleDto {
            session_id: dto.session_id,
            run_id: run.id,
            status: run.status,
        })
    }

    pub(super) async fn append_session_history(
        &self,
        run_id: &str,
        message: &AgentModelMessage,
    ) -> Result<(), ApplicationError> {
        let active = self.active_run_handle(run_id).await?;
        let Some(session_id) = active.target.session_id() else {
            return Ok(());
        };
        let entry = self
            .session_repository
            .append_session_message(session_id, run_id, message)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "session_message_appended",
            json!({
                "sessionId": session_id,
                "seq": entry.seq,
            }),
        )
        .await?;
        Ok(())
    }

    pub(super) async fn resolve_session_skills(
        &self,
        profile: &tt_domain::models::agent::profile::ResolvedAgentProfile,
    ) -> Result<Vec<tt_domain::models::skill::SkillIndexEntry>, ApplicationError> {
        if profile.skills.visible.is_empty() {
            return Ok(Vec::new());
        }
        self.skill_service
            .resolve_effective_skills(
                &[tt_domain::models::skill::SkillScope::Profile {
                    profile_id: profile.id.as_str().to_string(),
                }],
                &profile.skills,
            )
            .await
    }
}
