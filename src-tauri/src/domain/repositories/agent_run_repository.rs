use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentChatRef, AgentRun, AgentRunEvent, AgentRunEventLevel, AgentRunStatus,
    AgentRunSummaryProjection, ROOT_AGENT_INVOCATION_ID,
};

#[derive(Debug, Clone)]
pub struct AgentRunEventReadQuery {
    pub after_seq: Option<u64>,
    pub before_seq: Option<u64>,
    pub limit: usize,
    pub invocation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentRunListCursor {
    pub created_at: DateTime<Utc>,
    pub run_id: String,
}

#[derive(Debug, Clone)]
pub struct AgentRunListQuery {
    pub chat_ref: Option<AgentChatRef>,
    pub stable_chat_id: Option<String>,
    pub statuses: Option<Vec<AgentRunStatus>>,
    pub before: Option<AgentRunListCursor>,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentRunStorageEntryStats {
    pub file_count: usize,
    pub byte_count: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentRunStorageStats {
    pub total: AgentRunStorageEntryStats,
    pub heavy_artifacts: AgentRunStorageEntryStats,
}

#[async_trait]
pub trait AgentRunRepository: Send + Sync {
    async fn create_run(&self, run: &AgentRun) -> Result<(), DomainError>;

    async fn load_run(&self, run_id: &str) -> Result<AgentRun, DomainError>;

    async fn list_runs(&self, query: AgentRunListQuery) -> Result<Vec<AgentRun>, DomainError>;

    async fn list_all_runs(&self) -> Result<Vec<AgentRun>, DomainError>;

    async fn inspect_run_storage(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageStats, DomainError>;

    async fn load_run_summary_projection(
        &self,
        run_id: &str,
    ) -> Result<Option<AgentRunSummaryProjection>, DomainError>;

    async fn save_run_summary_projection(
        &self,
        projection: &AgentRunSummaryProjection,
    ) -> Result<(), DomainError>;

    async fn save_run(&self, run: &AgentRun) -> Result<(), DomainError>;

    async fn append_event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<AgentRunEvent, DomainError>;

    async fn read_events(
        &self,
        run_id: &str,
        query: AgentRunEventReadQuery,
    ) -> Result<Vec<AgentRunEvent>, DomainError>;

    async fn read_all_events(&self, run_id: &str) -> Result<Vec<AgentRunEvent>, DomainError>;
}

pub fn event_belongs_to_invocation(event: &AgentRunEvent, invocation_id: &str) -> bool {
    let payload = event.payload.as_object();
    let event_type = event.event_type.as_str();

    if let Some(scoped) = event_belongs_to_canonical_scope(payload, invocation_id) {
        return scoped;
    }

    if invocation_id == ROOT_AGENT_INVOCATION_ID {
        if event_type.starts_with("run_") {
            return true;
        }
        if event_type == "agent_delegate_started" {
            return payload_field(payload, "parentInvocationId") == ROOT_AGENT_INVOCATION_ID;
        }
        if event_type.starts_with("agent_task_") {
            return false;
        }
        let child_invocation_id = payload_field(payload, "childInvocationId");
        if !child_invocation_id.is_empty() && child_invocation_id != ROOT_AGENT_INVOCATION_ID {
            return false;
        }
        let new_invocation_id = payload_field(payload, "newInvocationId");
        if !new_invocation_id.is_empty() && new_invocation_id != ROOT_AGENT_INVOCATION_ID {
            return false;
        }
        return payload_field(payload, "invocationId") == ROOT_AGENT_INVOCATION_ID;
    }

    payload_field(payload, "invocationId") == invocation_id
        || payload_field(payload, "parentInvocationId") == invocation_id
        || payload_field(payload, "sourceInvocationId") == invocation_id
        || payload_field(payload, "childInvocationId") == invocation_id
        || payload_field(payload, "newInvocationId") == invocation_id
}

fn event_belongs_to_canonical_scope(
    payload: Option<&serde_json::Map<String, serde_json::Value>>,
    invocation_id: &str,
) -> Option<bool> {
    let scope = payload
        .and_then(|payload| payload.get("eventScope"))
        .and_then(|scope| scope.as_object())?;
    let scope_invocation_id = payload_field(Some(scope), "invocationId");
    let related_invocation_ids = scope
        .get("relatedInvocationIds")
        .and_then(|value| value.as_array());
    if scope_invocation_id.is_empty() && related_invocation_ids.is_none() {
        return None;
    }

    Some(
        scope_invocation_id == invocation_id
            || related_invocation_ids
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .any(|value| value == invocation_id),
    )
}

fn payload_field<'a>(
    payload: Option<&'a serde_json::Map<String, serde_json::Value>>,
    field: &str,
) -> &'a str {
    payload
        .and_then(|payload| payload.get(field))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::{Value, json};

    use super::*;

    fn event(event_type: &str, payload: Value) -> AgentRunEvent {
        AgentRunEvent {
            seq: 1,
            id: "evt_test".to_string(),
            run_id: "run_test".to_string(),
            timestamp: Utc::now(),
            level: AgentRunEventLevel::Info,
            event_type: event_type.to_string(),
            payload,
        }
    }

    #[test]
    fn canonical_event_scope_matches_primary_and_related_invocations() {
        let event = event(
            "agent_delegate_started",
            json!({
                "eventScope": {
                    "invocationId": "inv_parent",
                    "relatedInvocationIds": ["inv_child"]
                }
            }),
        );

        assert!(event_belongs_to_invocation(&event, "inv_parent"));
        assert!(event_belongs_to_invocation(&event, "inv_child"));
        assert!(!event_belongs_to_invocation(&event, "inv_other"));
    }

    #[test]
    fn legacy_invocation_fields_still_match_without_canonical_scope() {
        let parent_event = event(
            "agent_delegate_started",
            json!({
                "parentInvocationId": "inv_parent",
                "childInvocationId": "inv_child"
            }),
        );
        let source_event = event(
            "agent_handoff_accepted",
            json!({
                "sourceInvocationId": "inv_source",
                "newInvocationId": "inv_target"
            }),
        );

        assert!(event_belongs_to_invocation(&parent_event, "inv_parent"));
        assert!(event_belongs_to_invocation(&parent_event, "inv_child"));
        assert!(event_belongs_to_invocation(&source_event, "inv_source"));
        assert!(event_belongs_to_invocation(&source_event, "inv_target"));
    }
}
