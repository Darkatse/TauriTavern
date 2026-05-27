use serde_json::Value;

use crate::domain::models::agent::AgentTaskStatus;

pub(super) fn task_is_terminal(status: AgentTaskStatus) -> bool {
    matches!(
        status,
        AgentTaskStatus::Completed | AgentTaskStatus::Failed | AgentTaskStatus::Cancelled
    )
}

pub(super) fn task_return_status(value: Option<&Value>) -> Result<AgentTaskStatus, String> {
    let Some(value) = value else {
        return Ok(AgentTaskStatus::Completed);
    };
    let Some(value) = value.as_str() else {
        return Err("status must be a string".to_string());
    };
    match value.trim() {
        "" | "completed" => Ok(AgentTaskStatus::Completed),
        "failed" => Ok(AgentTaskStatus::Failed),
        other => Err(format!("unsupported status `{other}`")),
    }
}

pub(super) fn task_status_label(status: AgentTaskStatus) -> &'static str {
    match status {
        AgentTaskStatus::Queued => "queued",
        AgentTaskStatus::Running => "running",
        AgentTaskStatus::Completed => "completed",
        AgentTaskStatus::Failed => "failed",
        AgentTaskStatus::Cancelled => "cancelled",
    }
}
