use std::path::Path;

use serde_json::Value;
use tokio::fs::read_to_string;

use tt_adapter_storage_core::file_system::write_json_file;
use tt_contracts::agent_run_record::canonicalize_agent_run_record;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentRun;

pub(super) async fn write_agent_run_record(path: &Path, run: &AgentRun) -> Result<(), DomainError> {
    write_json_file(path, run).await
}

pub(super) async fn read_agent_run_record(path: &Path) -> Result<AgentRun, DomainError> {
    let contents = read_to_string(path).await.map_err(|error| {
        DomainError::file_io("read agent run", path.display().to_string(), error)
    })?;
    let mut value = serde_json::from_str::<Value>(&contents).map_err(|error| {
        DomainError::InvalidData(format!("Invalid JSON in {}: {}", path.display(), error))
    })?;
    canonicalize_agent_run_record(&mut value).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid agent run record in {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_value(value).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid agent run record in {}: {}",
            path.display(),
            error
        ))
    })
}
