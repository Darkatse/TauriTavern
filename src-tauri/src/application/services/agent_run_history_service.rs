use std::sync::Arc;

use crate::application::dto::agent_dto::{
    AgentListRunsCursorDto, AgentListRunsDto, AgentListRunsResultDto, AgentRunCommittedMessageDto,
    AgentRunSummaryDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AGENT_RUN_SUMMARY_PROJECTION_SCHEMA_VERSION, AgentRun, AgentRunCommittedMessageProjection,
    AgentRunEvent, AgentRunStatus, AgentRunSummaryProjection,
};
use crate::domain::repositories::agent_run_repository::{
    AgentRunListCursor, AgentRunListQuery, AgentRunRepository,
};

const MAX_AGENT_RUN_LIST_LIMIT: usize = 200;

pub struct AgentRunHistoryService {
    run_repository: Arc<dyn AgentRunRepository>,
}

impl AgentRunHistoryService {
    pub fn new(run_repository: Arc<dyn AgentRunRepository>) -> Self {
        Self { run_repository }
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
    is_terminal_run_status(run.status) && projection.terminal_at.is_some()
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

fn is_terminal_run_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "run_completed" | "run_partial_success" | "run_cancelled" | "run_failed"
    )
}

fn is_terminal_run_status(status: AgentRunStatus) -> bool {
    matches!(
        status,
        AgentRunStatus::Completed
            | AgentRunStatus::PartialSuccess
            | AgentRunStatus::Cancelled
            | AgentRunStatus::Failed
    )
}

fn normalize_limit(limit: usize) -> Result<usize, ApplicationError> {
    if limit == 0 || limit > MAX_AGENT_RUN_LIST_LIMIT {
        return Err(ApplicationError::ValidationError(format!(
            "agent.run_history_limit_invalid: limit must be between 1 and {MAX_AGENT_RUN_LIST_LIMIT}"
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
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    use super::*;
    use crate::domain::models::agent::{
        AgentChatRef, AgentRunEventLevel, AgentRunPresentation, AgentRunSkillScopeRefs,
        AgentRunStatus,
    };

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
}
