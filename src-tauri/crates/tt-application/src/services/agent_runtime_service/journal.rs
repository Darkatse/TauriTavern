use chrono::Utc;
use serde_json::{Map, Value, json};

use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::errors::ApplicationError;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{
    AgentRun, AgentRunEvent, AgentRunEventLevel, AgentRunStatus, WorkspacePath,
};

impl AgentRuntimeService {
    pub(super) async fn transition_status(
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

    pub(super) async fn event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<AgentRunEvent, ApplicationError> {
        let payload = with_canonical_event_scope(event_type, payload)?;
        self.run_repository
            .append_event(run_id, level, event_type, payload)
            .await
            .map_err(ApplicationError::from)
    }

    pub(super) fn ensure_not_cancelled(
        &self,
        cancel: &AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        if *cancel.borrow() {
            return Err(DomainError::generation_cancelled_by_user().into());
        }
        Ok(())
    }

    pub(super) async fn checkpoint_workspace_file(
        &self,
        run_id: &str,
        update_run_status: bool,
        reason: &str,
        event_type: &str,
        payload: Value,
        path: WorkspacePath,
    ) -> Result<(), ApplicationError> {
        if update_run_status {
            self.transition_status(run_id, AgentRunStatus::CreatingCheckpoint)
                .await?;
        }
        let event = self
            .event(run_id, AgentRunEventLevel::Info, event_type, payload)
            .await?;
        let checkpoint = self
            .checkpoint_repository
            .create_checkpoint(run_id, reason, event.seq, &[path])
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "checkpoint_created",
            json!({ "checkpointId": checkpoint.id, "reason": reason }),
        )
        .await?;
        Ok(())
    }
}

fn with_canonical_event_scope(event_type: &str, payload: Value) -> Result<Value, ApplicationError> {
    let mut payload = match payload {
        Value::Object(payload) => payload,
        payload => return Ok(payload),
    };

    let mut event_scope = match payload.remove("eventScope") {
        Some(Value::Object(event_scope)) => event_scope,
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "agent.event_scope_invalid: payload.eventScope must be an object".to_string(),
            ));
        }
        None => Map::new(),
    };

    let primary = event_primary_invocation_id(event_type, &payload, &event_scope);

    let Some(primary) = primary else {
        if !event_scope.is_empty() {
            payload.insert("eventScope".to_string(), Value::Object(event_scope));
        }
        return Ok(Value::Object(payload));
    };

    event_scope.insert("invocationId".to_string(), json!(primary.as_str()));

    let mut related = scope_related_invocation_ids(&event_scope);
    for field in [
        "invocationId",
        "parentInvocationId",
        "sourceInvocationId",
        "childInvocationId",
        "newInvocationId",
    ] {
        if let Some(invocation_id) = payload_string_field(&payload, field) {
            push_related_invocation(&mut related, invocation_id, &primary);
        }
    }
    if related.is_empty() {
        event_scope.remove("relatedInvocationIds");
    } else {
        event_scope.insert("relatedInvocationIds".to_string(), json!(related));
    }

    payload.insert("eventScope".to_string(), Value::Object(event_scope));
    Ok(Value::Object(payload))
}

fn scope_invocation_id(scope: &Map<String, Value>) -> Option<String> {
    scope
        .get("invocationId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn scope_related_invocation_ids(scope: &Map<String, Value>) -> Vec<String> {
    scope
        .get("relatedInvocationIds")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn payload_string_field(payload: &Map<String, Value>, field: &str) -> Option<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn event_primary_invocation_id(
    event_type: &str,
    payload: &Map<String, Value>,
    scope: &Map<String, Value>,
) -> Option<String> {
    scope_invocation_id(scope)
        .or_else(|| match event_type {
            "agent_delegate_started" => payload_string_field(payload, "parentInvocationId"),
            "agent_handoff_requested" | "agent_handoff_accepted" => {
                payload_string_field(payload, "sourceInvocationId")
            }
            "task_return_completed" => payload_string_field(payload, "childInvocationId"),
            event_type if event_type.starts_with("agent_task_") => {
                payload_string_field(payload, "parentInvocationId")
            }
            _ => None,
        })
        .or_else(|| payload_string_field(payload, "invocationId"))
        .or_else(|| payload_string_field(payload, "parentInvocationId"))
        .or_else(|| payload_string_field(payload, "sourceInvocationId"))
        .or_else(|| payload_string_field(payload, "childInvocationId"))
        .or_else(|| payload_string_field(payload, "newInvocationId"))
        .or_else(|| run_level_event_scope(event_type))
}

fn push_related_invocation(related: &mut Vec<String>, invocation_id: String, primary: &str) {
    if invocation_id == primary || related.iter().any(|existing| existing == &invocation_id) {
        return;
    }
    related.push(invocation_id);
}

fn run_level_event_scope(event_type: &str) -> Option<String> {
    if event_type.starts_with("run_") || event_type == "status_changed" {
        return Some(tt_domain::models::agent::ROOT_AGENT_INVOCATION_ID.to_string());
    }
    None
}
