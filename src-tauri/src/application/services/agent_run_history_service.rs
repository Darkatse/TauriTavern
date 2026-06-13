use std::collections::BTreeSet;
use std::sync::Arc;

use crate::application::dto::agent_dto::{
    AgentListRunsCursorDto, AgentListRunsDto, AgentListRunsResultDto, AgentPlanRunPruneDto,
    AgentRunCommittedMessageDto, AgentRunPruneActionDto, AgentRunPruneBlockReasonDto,
    AgentRunPruneBlockedRunDto, AgentRunPruneCandidateDto, AgentRunPrunePlanDto,
    AgentRunPruneReasonDto, AgentRunPruneRetentionDto, AgentRunSummaryDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_run_retention_planner::{
    AgentRunPruneAction, AgentRunPruneBlockReason, AgentRunPruneBlockedRun, AgentRunPruneCandidate,
    AgentRunPruneReason, AgentRunRetentionPlan, AgentRunRetentionPlanInput,
    AgentRunRetentionPlanner, MAX_AGENT_RUN_PRUNE_DETAIL_LIMIT, is_terminal_run_event,
};
use crate::application::services::agent_workspace_lifecycle_service::AgentRunActivity;
use crate::domain::models::agent::{
    AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION, AgentRun, AgentRunCommittedMessageProjection,
    AgentRunEvent, AgentRunSummaryProjection,
};
use crate::domain::models::settings::AgentRunRetentionSettings;
use crate::domain::repositories::agent_run_repository::{
    AgentRunListCursor, AgentRunListQuery, AgentRunRepository,
};
use crate::domain::repositories::settings_repository::SettingsRepository;

const MAX_AGENT_RUN_LIST_LIMIT: usize = 200;

pub struct AgentRunHistoryService {
    run_repository: Arc<dyn AgentRunRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    run_activity: Arc<dyn AgentRunActivity>,
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
        let active_run_ids = self
            .run_activity
            .active_run_ids()
            .await?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let planner = AgentRunRetentionPlanner::new(self.run_repository.as_ref());
        let plan = planner
            .plan(AgentRunRetentionPlanInput {
                retention,
                detail_limit,
                active_run_ids,
            })
            .await?;

        Ok(AgentRunPrunePlanDto::from_plan(plan))
    }

    async fn resolve_prune_retention(
        &self,
        dto: Option<AgentRunPruneRetentionDto>,
    ) -> Result<AgentRunRetentionSettings, ApplicationError> {
        let retention = if let Some(dto) = dto {
            AgentRunRetentionSettings {
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
    fn from_plan(plan: AgentRunRetentionPlan) -> Self {
        Self {
            retention: AgentRunPruneRetentionDto::from(plan.retention),
            detail_limit: plan.detail_limit,
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
    use std::path::PathBuf;
    use tokio::fs;

    use super::*;
    use crate::domain::models::agent::{
        AgentChatRef, AgentRunEventLevel, AgentRunPresentation, AgentRunSkillScopeRefs,
        AgentRunStatus,
    };
    use crate::domain::models::settings::{
        AgentRunRetentionSettings, AgentSettings, TauriTavernSettings,
    };
    use crate::domain::repositories::settings_repository::SettingsRepository;
    use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
    use crate::infrastructure::repositories::file_settings_repository::FileSettingsRepository;

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

    fn temp_root(label: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tauritavern-agent-run-history-{label}-{}-{suffix}",
            std::process::id()
        ))
    }

    async fn seed_heavy_file(root: &std::path::Path, run: &AgentRun, bytes: &[u8]) {
        let input_dir = root
            .join("chats")
            .join(&run.workspace_id)
            .join("runs")
            .join(&run.id)
            .join("input");
        fs::create_dir_all(&input_dir)
            .await
            .expect("create heavy input dir");
        fs::write(input_dir.join("prompt_snapshot.json"), bytes)
            .await
            .expect("write heavy input file");
    }

    async fn append_terminal_event(repository: &FileAgentRepository, run: &AgentRun) {
        let event_type = match run.status {
            AgentRunStatus::Completed => "run_completed",
            AgentRunStatus::PartialSuccess => "run_partial_success",
            AgentRunStatus::Cancelled => "run_cancelled",
            AgentRunStatus::Failed => "run_failed",
            _ => return,
        };
        repository
            .append_event(
                run.id.as_str(),
                AgentRunEventLevel::Info,
                event_type,
                Value::Null,
            )
            .await
            .expect("append terminal event");
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
        let agent_root = temp_root("agent");
        let settings_root = temp_root("settings");
        let run_repository = Arc::new(FileAgentRepository::new(agent_root.clone()));
        let settings_repository = Arc::new(FileSettingsRepository::new(settings_root.clone()));

        let mut settings = TauriTavernSettings::default();
        settings.agent = AgentSettings {
            retention: AgentRunRetentionSettings {
                keep_recent_terminal_runs: 2,
                keep_full_recent_runs: 1,
            },
        };
        settings_repository
            .save_tauritavern_settings(&settings)
            .await
            .expect("save settings");

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
            run_repository.create_run(run).await.expect("create run");
            append_terminal_event(&run_repository, run).await;
            seed_heavy_file(&agent_root, run, b"heavy").await;
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

        let _ = fs::remove_dir_all(agent_root).await;
        let _ = fs::remove_dir_all(settings_root).await;
    }

    #[tokio::test]
    async fn run_prune_plan_blocks_candidate_without_terminal_event() {
        let agent_root = temp_root("agent-missing-terminal");
        let settings_root = temp_root("settings-missing-terminal");
        let run_repository = Arc::new(FileAgentRepository::new(agent_root.clone()));
        let settings_repository = Arc::new(FileSettingsRepository::new(settings_root.clone()));
        let run = run_with_id(
            "run_prune_missing_terminal_event",
            instant("2026-01-04T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        run_repository.create_run(&run).await.expect("create run");
        seed_heavy_file(&agent_root, &run, b"heavy").await;

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

        let _ = fs::remove_dir_all(agent_root).await;
        let _ = fs::remove_dir_all(settings_root).await;
    }

    #[tokio::test]
    async fn run_prune_plan_blocks_terminal_run_that_is_still_active() {
        let agent_root = temp_root("agent-active-terminal");
        let settings_root = temp_root("settings-active-terminal");
        let run_repository = Arc::new(FileAgentRepository::new(agent_root.clone()));
        let settings_repository = Arc::new(FileSettingsRepository::new(settings_root.clone()));
        let run = run_with_id(
            "run_prune_active_terminal",
            instant("2026-01-04T00:00:00Z"),
            AgentRunStatus::Completed,
        );
        run_repository.create_run(&run).await.expect("create run");
        append_terminal_event(&run_repository, &run).await;
        seed_heavy_file(&agent_root, &run, b"heavy").await;

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

        let _ = fs::remove_dir_all(agent_root).await;
        let _ = fs::remove_dir_all(settings_root).await;
    }

    #[tokio::test]
    async fn run_prune_plan_detail_limit_does_not_truncate_totals() {
        let agent_root = temp_root("agent-detail-limit");
        let settings_root = temp_root("settings-detail-limit");
        let run_repository = Arc::new(FileAgentRepository::new(agent_root.clone()));
        let settings_repository = Arc::new(FileSettingsRepository::new(settings_root.clone()));
        for index in 0..3 {
            let run = run_with_id(
                &format!("run_prune_detail_limit_{index}"),
                instant(&format!("2026-01-0{}T00:00:00Z", index + 1)),
                AgentRunStatus::Completed,
            );
            run_repository.create_run(&run).await.expect("create run");
            append_terminal_event(&run_repository, &run).await;
            seed_heavy_file(&agent_root, &run, b"heavy").await;
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

        let _ = fs::remove_dir_all(agent_root).await;
        let _ = fs::remove_dir_all(settings_root).await;
    }
}
