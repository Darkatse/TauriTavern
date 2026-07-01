use std::path::Path;

use serde_json::{Value, json};
use tokio::fs::read_to_string;

use crate::infrastructure::persistence::file_system::write_json_file;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentRun;

pub(super) async fn write_agent_run_record(path: &Path, run: &AgentRun) -> Result<(), DomainError> {
    write_json_file(path, run).await
}

pub(super) async fn read_agent_run_record(path: &Path) -> Result<AgentRun, DomainError> {
    let contents = read_to_string(path).await.map_err(|error| {
        DomainError::InternalError(format!("Failed to read file {}: {}", path.display(), error))
    })?;
    let mut value = serde_json::from_str::<Value>(&contents).map_err(|error| {
        DomainError::InvalidData(format!("Invalid JSON in {}: {}", path.display(), error))
    })?;
    canonicalize_agent_run_record(&mut value, path)?;
    serde_json::from_value(value).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid agent run record in {}: {}",
            path.display(),
            error
        ))
    })
}

fn canonicalize_agent_run_record(value: &mut Value, path: &Path) -> Result<(), DomainError> {
    let object = value.as_object_mut().ok_or_else(|| {
        DomainError::InvalidData(format!(
            "Agent run record must be a JSON object: {}",
            path.display()
        ))
    })?;

    if !object.contains_key("presentation") {
        object.insert("presentation".to_string(), json!("foreground"));
    }
    if object.get("status").and_then(Value::as_str) == Some("awaiting_commit") {
        object.insert("status".to_string(), json!("awaiting_host_commit"));
    }

    Ok(())
}
