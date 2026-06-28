use std::collections::BTreeSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::application::dto::agent_dto::{
    AgentApplyRunPruneDto, AgentListRunsCursorDto, AgentListRunsDto, AgentListRunsResultDto,
    AgentPlanRunPruneDto, AgentRunCommittedMessageDto, AgentRunPruneActionDto,
    AgentRunPruneApplyResultDto, AgentRunPruneBlockReasonDto, AgentRunPruneBlockedRunDto,
    AgentRunPruneCandidateDto, AgentRunPruneFailedRunDto, AgentRunPrunePlanDto,
    AgentRunPruneReasonDto, AgentRunPruneRetentionDto, AgentRunSummaryDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_run_retention_planner::{
    AgentRunPruneAction, AgentRunPruneBlockReason, AgentRunPruneBlockedRun, AgentRunPruneCandidate,
    AgentRunPruneReason, AgentRunRetentionPlan, AgentRunRetentionPlanDetailMode,
    AgentRunRetentionPlanInput, AgentRunRetentionPlanner, MAX_AGENT_RUN_PRUNE_DETAIL_LIMIT,
    is_terminal_run_event,
};
use crate::application::services::agent_workspace_lifecycle_service::AgentRunActivity;
use crate::domain::models::agent::{
    AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION, AgentRun, AgentRunCommittedMessageProjection,
    AgentRunEvent, AgentRunSummaryProjection,
};
use crate::domain::models::settings::AgentRunRetentionSettings;
use crate::domain::repositories::agent_run_repository::{
    AgentRunListCursor, AgentRunListQuery, AgentRunRepository, AgentRunStorageEntryStats,
};
use crate::domain::repositories::settings_repository::SettingsRepository;

const MAX_AGENT_RUN_LIST_LIMIT: usize = 200;

pub struct AgentRunHistoryService {
    run_repository: Arc<dyn AgentRunRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    run_activity: Arc<dyn AgentRunActivity>,
    run_prune_apply_lock: Mutex<()>,
}

impl AgentRunHistoryService {
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        settings_repository: Arc<dyn SettingsRepository>,
        run_activity: Arc<dyn AgentRunActivity>,
    ) -> Self {
        Self {
            run_repository,
            settings_repository,
            run_activity,
            run_prune_apply_lock: Mutex::new(()),
        }
    }

    pub async fn list_runs(
        &self,
        dto: AgentListRunsDto,
    ) -> Result<AgentListRunsResultDto, ApplicationError> {
        let limit = normalize_limit(dto.limit)?;
        let mut runs = self
            .run_repository
            .list_runs(AgentRunListQuery {
                chat_ref: dto.chat_ref,
                stable_chat_id: normalize_optional_string(dto.stable_chat_id),
                statuses: if dto.statuses.is_empty() {
                    None
                } else {
                    Some(dto.statuses)
                },
                before: dto.before.map(normalize_cursor).transpose()?,
                limit: limit + 1,
            })
            .await?;

        let has_more = runs.len() > limit;
        if has_more {
            runs.truncate(limit);
        }

        let mut summaries = Vec::with_capacity(runs.len());
        for run in runs {
            let projection = self.summary_projection_for_run(&run).await?;
            summaries.push(AgentRunSummaryDto::from_run_and_projection(run, projection));
        }
        let next_cursor = if has_more {
            summaries.last().map(|run| AgentListRunsCursorDto {
                created_at: run.created_at,
                run_id: run.run_id.clone(),
            })
        } else {
            None
        };

        Ok(AgentListRunsResultDto {
            runs: summaries,
            next_cursor,
        })
    }

    pub async fn plan_run_prune(
        &self,
        dto: AgentPlanRunPruneDto,
    ) -> Result<AgentRunPrunePlanDto, ApplicationError> {
        let retention = self.resolve_prune_retention(dto.retention).await?;
        let detail_limit = normalize_prune_detail_limit(dto.detail_limit)?;
        let plan = self
            .build_preview_run_prune_plan(retention, detail_limit)
            .await?;

        Ok(AgentRunPrunePlanDto::from_preview_plan(plan, detail_limit))
    }

    pub async fn apply_run_prune(
        &self,
        dto: AgentApplyRunPruneDto,
    ) -> Result<AgentRunPruneApplyResultDto, ApplicationError> {
        let detail_limit = normalize_prune_detail_limit(dto.detail_limit)?;
        let _guard = self.run_prune_apply_lock.lock().await;
        self.apply_run_prune_locked(dto, detail_limit).await
    }

    pub(crate) async fn try_apply_run_prune_for_automation(
        &self,
        dto: AgentApplyRunPruneDto,
    ) -> Result<Option<AgentRunPruneApplyResultDto>, ApplicationError> {
        let detail_limit = normalize_prune_detail_limit(dto.detail_limit)?;
        let Ok(_guard) = self.run_prune_apply_lock.try_lock() else {
            return Ok(None);
        };
        self.apply_run_prune_locked(dto, detail_limit)
            .await
            .map(Some)
    }

    async fn apply_run_prune_locked(
        &self,
        dto: AgentApplyRunPruneDto,
        detail_limit: usize,
    ) -> Result<AgentRunPruneApplyResultDto, ApplicationError> {
        let retention = self.resolve_prune_retention(dto.retention).await?;
        let retention_dto = AgentRunPruneRetentionDto::from(retention.clone());
        let execution_plan = self
            .build_execution_run_prune_plan(retention.clone())
            .await?;

        let mut slimmed_run_count = 0;
        let mut deleted_run_count = 0;
        let mut failed_run_count = 0;
        let mut removed_file_count = 0;
        let mut removed_byte_count = 0;
        let mut failed_details_truncated = false;
        let mut failed_runs = Vec::new();

        for candidate in execution_plan.candidates {
            let result = match candidate.action {
                AgentRunPruneAction::SlimHeavyArtifacts => {
                    self.run_repository
                        .slim_run_heavy_artifacts(&candidate.run)
                        .await
                }
                AgentRunPruneAction::DeleteRun => {
                    self.run_repository.delete_run(&candidate.run).await
                }
            };

            match result {
                Ok(stats) => {
                    match candidate.action {
                        AgentRunPruneAction::SlimHeavyArtifacts => slimmed_run_count += 1,
                        AgentRunPruneAction::DeleteRun => deleted_run_count += 1,
                    }
                    accumulate_apply_stats(
                        &mut removed_file_count,
                        &mut removed_byte_count,
                        stats,
                    )?;
                }
                Err(error) => {
                    failed_run_count += 1;
                    if failed_runs.len() < detail_limit {
                        failed_runs.push(AgentRunPruneFailedRunDto::from_candidate(
                            candidate,
                            error.to_string(),
                        ));
                    } else {
                        failed_details_truncated = true;
                    }
                }
            }
        }

        let after_plan = self
            .build_preview_run_prune_plan(retention, detail_limit)
            .await
            .map(|plan| AgentRunPrunePlanDto::from_preview_plan(plan, detail_limit))?;

        Ok(AgentRunPruneApplyResultDto {
            retention: retention_dto,
            detail_limit,
            slimmed_run_count,
            deleted_run_count,
            failed_run_count,
            removed_file_count,
            removed_byte_count,
            failed_details_truncated,
            failed_runs,
            after_plan,
        })
    }

    async fn resolve_prune_retention(
        &self,
        dto: Option<AgentRunPruneRetentionDto>,
    ) -> Result<AgentRunRetentionSettings, ApplicationError> {
        let retention = if let Some(dto) = dto {
            AgentRunRetentionSettings {
                auto_prune_enabled: false,
                keep_recent_terminal_runs: dto.keep_recent_terminal_runs,
                keep_full_recent_runs: dto.keep_full_recent_runs,
            }
        } else {
            self.settings_repository
                .load_tauritavern_settings()
                .await?
                .agent
                .retention
        };

        validate_prune_retention(&retention)?;
        Ok(retention)
    }

    async fn build_preview_run_prune_plan(
        &self,
        retention: AgentRunRetentionSettings,
        detail_limit: usize,
    ) -> Result<AgentRunRetentionPlan, ApplicationError> {
        self.build_run_prune_plan(
            retention,
            AgentRunRetentionPlanDetailMode::Preview { detail_limit },
        )
        .await
    }

    async fn build_execution_run_prune_plan(
        &self,
        retention: AgentRunRetentionSettings,
    ) -> Result<AgentRunRetentionPlan, ApplicationError> {
        self.build_run_prune_plan(retention, AgentRunRetentionPlanDetailMode::Execution)
            .await
    }

    async fn build_run_prune_plan(
        &self,
        retention: AgentRunRetentionSettings,
        detail_mode: AgentRunRetentionPlanDetailMode,
    ) -> Result<AgentRunRetentionPlan, ApplicationError> {
        let active_run_ids = self.active_run_id_set().await?;
        let planner = AgentRunRetentionPlanner::new(self.run_repository.as_ref());
        planner
            .plan(AgentRunRetentionPlanInput {
                retention,
                detail_mode,
                active_run_ids,
            })
            .await
    }

    async fn active_run_id_set(&self) -> Result<BTreeSet<String>, ApplicationError> {
        Ok(self
            .run_activity
            .active_run_ids()
            .await?
            .into_iter()
            .collect())
    }

    async fn summary_projection_for_run(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunSummaryProjection, ApplicationError> {
        if let Some(projection) = self
            .run_repository
            .load_run_summary_projection(&run.id)
            .await?
            && projection_is_current(&projection, run)
        {
            return Ok(projection);
        }

        let events = self.run_repository.read_all_events(&run.id).await?;
        let projection = build_summary_projection(run, &events);
        if projection_can_be_cached(run, &projection) {
            self.run_repository
                .save_run_summary_projection(&projection)
                .await?;
        }
        Ok(projection)
    }
}

impl From<AgentRunRetentionSettings> for AgentRunPruneRetentionDto {
    fn from(retention: AgentRunRetentionSettings) -> Self {
        Self {
            keep_recent_terminal_runs: retention.keep_recent_terminal_runs,
            keep_full_recent_runs: retention.keep_full_recent_runs,
        }
    }
}

impl AgentRunPrunePlanDto {
    fn from_preview_plan(plan: AgentRunRetentionPlan, detail_limit: usize) -> Self {
        Self {
            retention: AgentRunPruneRetentionDto::from(plan.retention),
            detail_limit,
            terminal_run_count: plan.terminal_run_count,
            non_terminal_run_count: plan.non_terminal_run_count,
            blocked_run_count: plan.blocked_run_count,
            full_retained_run_count: plan.full_retained_run_count,
            core_retained_run_count: plan.core_retained_run_count,
            slim_candidate_count: plan.slim_candidate_count,
            delete_candidate_count: plan.delete_candidate_count,
            total_slim_file_count: plan.total_slim.file_count,
            total_slim_byte_count: plan.total_slim.byte_count,
            total_delete_file_count: plan.total_delete.file_count,
            total_delete_byte_count: plan.total_delete.byte_count,
            total_candidate_file_count: plan.total_candidate.file_count,
            total_candidate_byte_count: plan.total_candidate.byte_count,
            candidate_details_truncated: plan.candidate_details_truncated,
            candidates: plan
                .candidates
                .into_iter()
                .map(AgentRunPruneCandidateDto::from_candidate)
                .collect(),
            blocked_details_truncated: plan.blocked_details_truncated,
            blocked_runs: plan
                .blocked_runs
                .into_iter()
                .map(AgentRunPruneBlockedRunDto::from_blocked_run)
                .collect(),
        }
    }
}

impl AgentRunPruneCandidateDto {
    fn from_candidate(candidate: AgentRunPruneCandidate) -> Self {
        Self {
            run_id: candidate.run.id,
            workspace_id: candidate.run.workspace_id,
            stable_chat_id: candidate.run.stable_chat_id,
            chat_ref: candidate.run.chat_ref,
            status: candidate.run.status,
            created_at: candidate.run.created_at,
            updated_at: candidate.run.updated_at,
            action: AgentRunPruneActionDto::from(candidate.action),
            reason: AgentRunPruneReasonDto::from(candidate.reason),
            file_count: candidate.stats.file_count,
            byte_count: candidate.stats.byte_count,
        }
    }
}

impl AgentRunPruneBlockedRunDto {
    fn from_blocked_run(blocked: AgentRunPruneBlockedRun) -> Self {
        Self {
            run_id: blocked.run.id,
            workspace_id: blocked.run.workspace_id,
            stable_chat_id: blocked.run.stable_chat_id,
            chat_ref: blocked.run.chat_ref,
            status: blocked.run.status,
            created_at: blocked.run.created_at,
            updated_at: blocked.run.updated_at,
            action: AgentRunPruneActionDto::from(blocked.action),
            reason: AgentRunPruneReasonDto::from(blocked.reason),
            block_reason: AgentRunPruneBlockReasonDto::from(blocked.block_reason),
            message: blocked.message,
        }
    }
}

impl AgentRunPruneFailedRunDto {
    fn from_candidate(candidate: AgentRunPruneCandidate, message: String) -> Self {
        Self {
            run_id: candidate.run.id,
            workspace_id: candidate.run.workspace_id,
            stable_chat_id: candidate.run.stable_chat_id,
            chat_ref: candidate.run.chat_ref,
            status: candidate.run.status,
            created_at: candidate.run.created_at,
            updated_at: candidate.run.updated_at,
            action: AgentRunPruneActionDto::from(candidate.action),
            reason: AgentRunPruneReasonDto::from(candidate.reason),
            file_count: candidate.stats.file_count,
            byte_count: candidate.stats.byte_count,
            message,
        }
    }
}

impl From<AgentRunPruneAction> for AgentRunPruneActionDto {
    fn from(action: AgentRunPruneAction) -> Self {
        match action {
            AgentRunPruneAction::SlimHeavyArtifacts => Self::SlimHeavyArtifacts,
            AgentRunPruneAction::DeleteRun => Self::DeleteRun,
        }
    }
}

impl From<AgentRunPruneReason> for AgentRunPruneReasonDto {
    fn from(reason: AgentRunPruneReason) -> Self {
        match reason {
            AgentRunPruneReason::OutsideFullRetentionWindow => Self::OutsideFullRetentionWindow,
            AgentRunPruneReason::OutsideHistoryRetentionWindow => {
                Self::OutsideHistoryRetentionWindow
            }
        }
    }
}

impl From<AgentRunPruneBlockReason> for AgentRunPruneBlockReasonDto {
    fn from(reason: AgentRunPruneBlockReason) -> Self {
        match reason {
            AgentRunPruneBlockReason::ActiveRun => Self::ActiveRun,
            AgentRunPruneBlockReason::MissingTerminalEvent => Self::MissingTerminalEvent,
            AgentRunPruneBlockReason::InvalidJournal => Self::InvalidJournal,
            AgentRunPruneBlockReason::InvalidStorage => Self::InvalidStorage,
        }
    }
}

impl AgentRunSummaryDto {
    fn from_run_and_projection(run: AgentRun, projection: AgentRunSummaryProjection) -> Self {
        Self {
            run_id: run.id,
            workspace_id: run.workspace_id,
            stable_chat_id: run.stable_chat_id,
            chat_ref: run.chat_ref,
            generation_type: run.generation_type,
            profile_id: run.profile_id,
            skill_scope_refs: run.skill_scope_refs,
            persist_base_state_id: run.persist_base_state_id,
            input_message_count: run.input_message_count,
            presentation: run.presentation,
            status: run.status,
            created_at: run.created_at,
            updated_at: run.updated_at,
            commit_count: projection.commit_count,
            committed_message: projection
                .committed_message
                .map(AgentRunCommittedMessageDto::from),
            terminal_at: projection.terminal_at,
        }
    }
}

impl From<AgentRunCommittedMessageProjection> for AgentRunCommittedMessageDto {
    fn from(message: AgentRunCommittedMessageProjection) -> Self {
        Self {
            commit_id: message.commit_id,
            message_id: message.message_id,
            message_index: message.message_index,
            committed_at: message.committed_at,
        }
    }
}

fn projection_is_current(projection: &AgentRunSummaryProjection, run: &AgentRun) -> bool {
    projection.schema_version == AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION
        && projection.run_id == run.id
        && projection.source_run_updated_at == run.updated_at
        && projection_can_be_cached(run, projection)
}

fn projection_can_be_cached(run: &AgentRun, projection: &AgentRunSummaryProjection) -> bool {
    run.status.is_terminal() && projection.terminal_at.is_some()
}

fn build_summary_projection(run: &AgentRun, events: &[AgentRunEvent]) -> AgentRunSummaryProjection {
    let mut commit_count = 0usize;
    let mut committed_message = None;
    let mut terminal_at = None;

    for event in events {
        if event.event_type == "chat_commit_completed" {
            commit_count += 1;
            if let Some(message_id) = payload_text(&event.payload, "messageId") {
                let message_index = payload_usize(&event.payload, "messageIndex")
                    .or_else(|| parse_message_index(&message_id));
                committed_message = Some(AgentRunCommittedMessageProjection {
                    commit_id: payload_text(&event.payload, "commitId")
                        .unwrap_or_else(|| event.id.clone()),
                    message_id,
                    message_index,
                    committed_at: event.timestamp,
                });
            }
            continue;
        }

        if is_terminal_run_event(&event.event_type) {
            terminal_at = Some(event.timestamp);
        }
    }

    AgentRunSummaryProjection {
        schema_version: AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION,
        run_id: run.id.clone(),
        source_run_updated_at: run.updated_at,
        commit_count,
        committed_message,
        terminal_at,
    }
}

fn payload_text(payload: &serde_json::Value, key: &str) -> Option<String> {
    let value = payload.get(key)?;
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
    if let Some(number) = value.as_u64() {
        return Some(number.to_string());
    }
    None
}

fn payload_usize(payload: &serde_json::Value, key: &str) -> Option<usize> {
    let value = payload.get(key)?;
    if let Some(number) = value.as_u64() {
        return usize::try_from(number).ok();
    }
    value
        .as_str()
        .and_then(|text| text.trim().parse::<usize>().ok())
}

fn parse_message_index(message_id: &str) -> Option<usize> {
    message_id.trim().parse::<usize>().ok()
}

fn validate_prune_retention(retention: &AgentRunRetentionSettings) -> Result<(), ApplicationError> {
    retention
        .validate()
        .map_err(|error| ApplicationError::ValidationError(error.message()))
}

fn normalize_limit(limit: usize) -> Result<usize, ApplicationError> {
    if limit == 0 || limit > MAX_AGENT_RUN_LIST_LIMIT {
        return Err(ApplicationError::ValidationError(format!(
            "agent.run_history_limit_invalid: limit must be between 1 and {MAX_AGENT_RUN_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

fn normalize_prune_detail_limit(limit: usize) -> Result<usize, ApplicationError> {
    if limit > MAX_AGENT_RUN_PRUNE_DETAIL_LIMIT {
        return Err(ApplicationError::ValidationError(format!(
            "agent.run_prune_detail_limit_invalid: detailLimit must be between 0 and {MAX_AGENT_RUN_PRUNE_DETAIL_LIMIT}"
        )));
    }
    Ok(limit)
}

fn accumulate_apply_stats(
    file_count: &mut usize,
    byte_count: &mut u64,
    stats: AgentRunStorageEntryStats,
) -> Result<(), ApplicationError> {
    *file_count = file_count.checked_add(stats.file_count).ok_or_else(|| {
        ApplicationError::InternalError("agent.run_prune_file_count_overflow".to_string())
    })?;
    *byte_count = byte_count.checked_add(stats.byte_count).ok_or_else(|| {
        ApplicationError::InternalError("agent.run_prune_byte_count_overflow".to_string())
    })?;
    Ok(())
}

fn normalize_cursor(
    cursor: AgentListRunsCursorDto,
) -> Result<AgentRunListCursor, ApplicationError> {
    let run_id = cursor.run_id.trim();
    if run_id.is_empty() {
        return Err(ApplicationError::ValidationError(
            "agent.run_history_cursor_invalid: before.runId is required".to_string(),
        ));
    }
    Ok(AgentRunListCursor {
        created_at: cursor.created_at,
        run_id: run_id.to_string(),
    })
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    use super::*;
    use crate::application::services::agent_run_retention_test_support::{
        TestAgentRunRepository, TestSettingsRepository,
    };
    use crate::domain::models::agent::{
        AgentChatRef, AgentRunEventLevel, AgentRunPresentation, AgentRunSkillScopeRefs,
        AgentRunStatus,
    };
    use crate::domain::models::settings::{
        AgentRunRetentionSettings, AgentSettings, TauriTavernSettings,
    };
    use crate::domain::repositories::agent_run_repository::AgentRunRepository;

    struct TestRunActivity {
        active_run_ids: Vec<String>,
    }

    impl TestRunActivity {
        fn none() -> Arc<Self> {
            Arc::new(Self {
                active_run_ids: Vec::new(),
            })
        }

        fn with_active(run_ids: Vec<String>) -> Arc<Self> {
            Arc::new(Self {
                active_run_ids: run_ids,
            })
        }
    }

    #[async_trait]
    impl AgentRunActivity for TestRunActivity {
        async fn active_run_ids(&self) -> Result<Vec<String>, ApplicationError> {
            Ok(self.active_run_ids.clone())
        }

        async fn active_run_ids_for_workspace(
            &self,
            _workspace_id: &str,
        ) -> Result<Vec<String>, ApplicationError> {
            Ok(self.active_run_ids.clone())
        }
    }

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    fn run() -> AgentRun {
        AgentRun {
            id: "run_summary_test".to_string(),
            workspace_id: "chat_summary_test".to_string(),
            stable_chat_id: "stable_summary_test".to_string(),
            chat_ref: AgentChatRef::Character {
                character_id: "Seraphina".to_string(),
                file_name: "Seraphina.png".to_string(),
            },
            generation_type: "normal".to_string(),
            profile_id: Some("writer".to_string()),
            skill_scope_refs: AgentRunSkillScopeRefs::default(),
            persist_base_state_id: None,
            input_message_count: Some(12),
            presentation: AgentRunPresentation::Background,
            status: AgentRunStatus::Completed,
            created_at: instant("2026-01-01T00:00:00Z"),
            updated_at: instant("2026-01-01T00:05:00Z"),
        }
    }

    fn run_with_id(id: &str, created_at: DateTime<Utc>, status: AgentRunStatus) -> AgentRun {
        AgentRun {
            id: id.to_string(),
            created_at,
            updated_at: created_at,
            status,
            ..run()
        }
    }

    fn event(seq: u64, event_type: &str, timestamp: &str, payload: Value) -> AgentRunEvent {
        AgentRunEvent {
            seq,
            id: format!("evt_{seq}"),
            run_id: "run_summary_test".to_string(),
            timestamp: instant(timestamp),
            level: AgentRunEventLevel::Info,
            event_type: event_type.to_string(),
            payload,
        }
    }

    async fn seed_run(
        repository: &TestAgentRunRepository,
        run: &AgentRun,
        heavy_artifact_bytes: &[u8],
    ) {
        repository.create_run(run).await.expect("create run");
        repository.append_terminal_event_for_run(run).await;
        repository
            .add_heavy_artifact(run, heavy_artifact_bytes.len() as u64)
            .await;
    }

    async fn seed_run_without_terminal_event(
        repository: &TestAgentRunRepository,
        run: &AgentRun,
        heavy_artifact_bytes: &[u8],
    ) {
        repository.create_run(run).await.expect("create run");
        repository
            .add_heavy_artifact(run, heavy_artifact_bytes.len() as u64)
            .await;
    }

    #[test]
    fn summary_projection_extracts_committed_message_index_from_message_id() {
        let projection = build_summary_projection(
            &run(),
            &[
                event(
                    1,
                    "chat_commit_completed",
                    "2026-01-01T00:02:00Z",
                    json!({
                        "commitId": "commit_a",
                        "messageId": "7"
                    }),
                ),
                event(2, "run_completed", "2026-01-01T00:03:00Z", Value::Null),
            ],
        );

        assert_eq!(projection.commit_count, 1);
        assert_eq!(
            projection.terminal_at,
            Some(instant("2026-01-01T00:03:00Z"))
        );
        let committed = projection
            .committed_message
            .expect("committed message projection");
        assert_eq!(committed.commit_id, "commit_a");
        assert_eq!(committed.message_id, "7");
        assert_eq!(committed.message_index, Some(7));
        assert_eq!(committed.committed_at, instant("2026-01-01T00:02:00Z"));
    }

    #[test]
    fn summary_projection_cache_reusable_only_after_terminal_event() {
        let mut run = run();
        run.status = AgentRunStatus::Completed;
        let projection = build_summary_projection(
            &run,
            &[event(
                1,
                "run_completed",
                "2026-01-01T00:03:00Z",
                Value::Null,
            )],
        );
        assert!(projection_is_current(&projection, &run));

        let incomplete_projection = build_summary_projection(&run, &[]);
        assert!(!projection_is_current(&incomplete_projection, &run));

        let mut active_run = run;
        active_run.status = AgentRunStatus::DispatchingTool;
        assert!(!projection_is_current(&projection, &active_run));
    }

    #[test]
    fn summary_projection_omits_locator_when_commit_has_no_message_id() {
        let projection = build_summary_projection(
            &run(),
            &[event(
                1,
                "chat_commit_completed",
                "2026-01-01T00:02:00Z",
                json!({
                    "commitId": "commit_old"
                }),
            )],
        );

        assert_eq!(projection.commit_count, 1);
        assert!(projection.committed_message.is_none());
    }

    #[tokio::test]
    async fn run_prune_plan_uses_settings_retention_windows() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();

        let mut settings = TauriTavernSettings::default();
        settings.agent = AgentSettings {
            retention: AgentRunRetentionSettings {
                auto_prune_enabled: false,
                keep_recent_terminal_runs: 2,
                keep_full_recent_runs: 1,
            },
        };
        settings_repository
            .store_tauritavern_settings(settings)
            .await;

        let newest = run_with_id(
            "run_prune_newest",
            instant("2026-01-04T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        let middle = run_with_id(
            "run_prune_middle",
            instant("2026-01-03T00:00:00Z"),
            AgentRunStatus::Failed,
        );
        let oldest = run_with_id(
            "run_prune_oldest",
            instant("2026-01-02T00:00:00Z"),
            AgentRunStatus::Cancelled,
        );
        let active = run_with_id(
            "run_prune_active",
            instant("2026-01-01T00:00:00Z"),
            AgentRunStatus::CallingModel,
        );
        for run in [&newest, &middle, &oldest, &active] {
            seed_run(&run_repository, run, b"heavy").await;
        }

        let service = AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::none(),
        );
        let plan = service
            .plan_run_prune(AgentPlanRunPruneDto {
                retention: None,
                detail_limit: 200,
            })
            .await
            .expect("plan prune");

        assert_eq!(plan.terminal_run_count, 3);
        assert_eq!(plan.non_terminal_run_count, 1);
        assert_eq!(plan.blocked_run_count, 0);
        assert_eq!(plan.full_retained_run_count, 1);
        assert_eq!(plan.core_retained_run_count, 1);
        assert_eq!(plan.slim_candidate_count, 1);
        assert_eq!(plan.delete_candidate_count, 1);
        assert_eq!(plan.total_slim_file_count, 1);
        assert_eq!(plan.total_slim_byte_count, 5);
        assert_eq!(
            plan.candidates
                .iter()
                .map(|candidate| (candidate.run_id.as_str(), candidate.action))
                .collect::<Vec<_>>(),
            vec![
                (
                    "run_prune_middle",
                    AgentRunPruneActionDto::SlimHeavyArtifacts
                ),
                ("run_prune_oldest", AgentRunPruneActionDto::DeleteRun),
            ]
        );
    }

    #[tokio::test]
    async fn run_prune_plan_ranks_terminal_runs_by_terminal_event_time() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();

        let mut created_newer_terminal_older = run_with_id(
            "run_prune_created_newer_terminal_older",
            instant("2026-01-10T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        created_newer_terminal_older.updated_at = instant("2026-01-10T00:05:00Z");

        let mut created_older_terminal_newer = run_with_id(
            "run_prune_created_older_terminal_newer",
            instant("2026-01-01T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        created_older_terminal_newer.updated_at = instant("2026-01-11T00:05:00Z");

        for run in [&created_newer_terminal_older, &created_older_terminal_newer] {
            seed_run(&run_repository, run, b"heavy").await;
        }

        let service = AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::none(),
        );
        let plan = service
            .plan_run_prune(AgentPlanRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto {
                    keep_recent_terminal_runs: 1,
                    keep_full_recent_runs: 1,
                }),
                detail_limit: 200,
            })
            .await
            .expect("plan prune");

        assert_eq!(plan.terminal_run_count, 2);
        assert_eq!(plan.full_retained_run_count, 1);
        assert_eq!(plan.delete_candidate_count, 1);
        assert_eq!(
            plan.candidates
                .iter()
                .map(|candidate| candidate.run_id.as_str())
                .collect::<Vec<_>>(),
            vec!["run_prune_created_newer_terminal_older"]
        );
    }

    #[tokio::test]
    async fn run_prune_plan_blocks_candidate_without_terminal_event() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();
        let run = run_with_id(
            "run_prune_missing_terminal_event",
            instant("2026-01-04T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        seed_run_without_terminal_event(&run_repository, &run, b"heavy").await;

        let service = AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::none(),
        );
        let plan = service
            .plan_run_prune(AgentPlanRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto {
                    keep_recent_terminal_runs: 0,
                    keep_full_recent_runs: 0,
                }),
                detail_limit: 200,
            })
            .await
            .expect("plan prune");

        assert_eq!(plan.terminal_run_count, 1);
        assert_eq!(plan.delete_candidate_count, 0);
        assert_eq!(plan.blocked_run_count, 1);
        assert_eq!(plan.blocked_runs.len(), 1);
        let blocked = &plan.blocked_runs[0];
        assert_eq!(blocked.run_id, "run_prune_missing_terminal_event");
        assert_eq!(blocked.action, AgentRunPruneActionDto::DeleteRun);
        assert_eq!(
            blocked.block_reason,
            AgentRunPruneBlockReasonDto::MissingTerminalEvent
        );
    }

    #[tokio::test]
    async fn run_prune_plan_blocks_terminal_run_that_is_still_active() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();
        let run = run_with_id(
            "run_prune_active_terminal",
            instant("2026-01-04T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        seed_run(&run_repository, &run, b"heavy").await;

        let service = AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::with_active(vec![run.id.clone()]),
        );
        let plan = service
            .plan_run_prune(AgentPlanRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto {
                    keep_recent_terminal_runs: 0,
                    keep_full_recent_runs: 0,
                }),
                detail_limit: 200,
            })
            .await
            .expect("plan prune");

        assert_eq!(plan.delete_candidate_count, 0);
        assert_eq!(plan.blocked_run_count, 1);
        assert_eq!(plan.blocked_runs.len(), 1);
        let blocked = &plan.blocked_runs[0];
        assert_eq!(blocked.run_id, "run_prune_active_terminal");
        assert_eq!(blocked.block_reason, AgentRunPruneBlockReasonDto::ActiveRun);
    }

    #[tokio::test]
    async fn run_prune_plan_detail_limit_does_not_truncate_totals() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();
        for index in 0..3 {
            let run = run_with_id(
                &format!("run_prune_detail_limit_{index}"),
                instant(&format!("2026-01-0{}T00:00:00Z", index + 1)),
                AgentRunStatus::Completed,
            );
            seed_run(&run_repository, &run, b"heavy").await;
        }

        let service = AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::none(),
        );
        let plan = service
            .plan_run_prune(AgentPlanRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto {
                    keep_recent_terminal_runs: 0,
                    keep_full_recent_runs: 0,
                }),
                detail_limit: 1,
            })
            .await
            .expect("plan prune");

        assert_eq!(plan.delete_candidate_count, 3);
        assert_eq!(plan.candidates.len(), 1);
        assert!(plan.candidate_details_truncated);
        assert!(plan.total_candidate_file_count >= 3);
    }

    #[tokio::test]
    async fn apply_run_prune_executes_all_candidates_when_detail_limit_truncates() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();
        for index in 0..3 {
            let run = run_with_id(
                &format!("run_prune_apply_detail_limit_{index}"),
                instant(&format!("2026-01-0{}T00:00:00Z", index + 1)),
                AgentRunStatus::Completed,
            );
            seed_run(&run_repository, &run, b"heavy").await;
        }

        let service = AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::none(),
        );
        let result = service
            .apply_run_prune(AgentApplyRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto {
                    keep_recent_terminal_runs: 0,
                    keep_full_recent_runs: 0,
                }),
                detail_limit: 1,
            })
            .await
            .expect("apply prune");

        assert_eq!(result.deleted_run_count, 3);
        assert_eq!(result.failed_run_count, 0);
        assert!(result.removed_file_count >= 3);
        assert_eq!(result.after_plan.delete_candidate_count, 0);
        assert_eq!(result.after_plan.candidates.len(), 0);
    }

    #[tokio::test]
    async fn apply_run_prune_serializes_concurrent_calls_without_false_failures() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();
        let run = run_with_id(
            "run_prune_apply_concurrent",
            instant("2026-01-04T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        seed_run(&run_repository, &run, b"heavy").await;

        let service = Arc::new(AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::none(),
        ));
        let dto = AgentApplyRunPruneDto {
            retention: Some(AgentRunPruneRetentionDto {
                keep_recent_terminal_runs: 0,
                keep_full_recent_runs: 0,
            }),
            detail_limit: 8,
        };
        let left_service = service.clone();
        let right_service = service.clone();
        let left_dto = dto.clone();
        let (left, right) = tokio::join!(
            async move { left_service.apply_run_prune(left_dto).await },
            async move { right_service.apply_run_prune(dto).await },
        );
        let left = left.expect("left apply prune");
        let right = right.expect("right apply prune");

        assert_eq!(left.failed_run_count + right.failed_run_count, 0);
        assert_eq!(left.deleted_run_count + right.deleted_run_count, 1);
        assert_eq!(left.after_plan.delete_candidate_count, 0);
        assert_eq!(right.after_plan.delete_candidate_count, 0);
    }

    #[tokio::test]
    async fn try_apply_run_prune_for_automation_skips_when_apply_lock_is_held() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();
        let service = AgentRunHistoryService::new(
            run_repository,
            settings_repository,
            TestRunActivity::none(),
        );

        let _guard = service.run_prune_apply_lock.lock().await;
        let result = service
            .try_apply_run_prune_for_automation(AgentApplyRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto {
                    keep_recent_terminal_runs: 0,
                    keep_full_recent_runs: 0,
                }),
                detail_limit: 8,
            })
            .await
            .expect("try apply should not fail");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn apply_run_prune_slims_core_window_and_deletes_history_window() {
        let run_repository = TestAgentRunRepository::new();
        let settings_repository = TestSettingsRepository::new();
        let newest = run_with_id(
            "run_prune_apply_newest",
            instant("2026-01-04T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        let middle = run_with_id(
            "run_prune_apply_middle",
            instant("2026-01-03T00:00:00Z"),
            AgentRunStatus::Failed,
        );
        let oldest = run_with_id(
            "run_prune_apply_oldest",
            instant("2026-01-02T00:00:00Z"),
            AgentRunStatus::Cancelled,
        );
        for run in [&newest, &middle, &oldest] {
            seed_run(&run_repository, run, b"heavy").await;
        }

        let service = AgentRunHistoryService::new(
            run_repository.clone(),
            settings_repository,
            TestRunActivity::none(),
        );
        let result = service
            .apply_run_prune(AgentApplyRunPruneDto {
                retention: Some(AgentRunPruneRetentionDto {
                    keep_recent_terminal_runs: 2,
                    keep_full_recent_runs: 1,
                }),
                detail_limit: 8,
            })
            .await
            .expect("apply prune");

        assert_eq!(result.slimmed_run_count, 1);
        assert_eq!(result.deleted_run_count, 1);
        assert_eq!(result.failed_run_count, 0);
        assert_eq!(result.after_plan.slim_candidate_count, 0);
        assert_eq!(result.after_plan.delete_candidate_count, 0);
        assert!(run_repository.has_run(&newest.id).await);
        assert_eq!(run_repository.heavy_artifact_count(&newest.id).await, 1);
        assert!(run_repository.has_run(&middle.id).await);
        assert_eq!(run_repository.heavy_artifact_count(&middle.id).await, 0);
        assert!(!run_repository.has_run(&oldest.id).await);
    }
}
