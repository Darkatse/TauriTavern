use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::FileAgentRepository;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentRun, AgentRunEvent, AgentRunEventLevel};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};

#[async_trait]
impl AgentRunRepository for FileAgentRepository {
    async fn create_run(&self, run: &AgentRun) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        fs::create_dir_all(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create agent run directory {}: {}",
                run_dir.display(),
                error
            ))
        })?;

        Self::write_json_atomic(&run_dir.join("run.json"), run).await?;
        Self::write_json_atomic(&self.index_run_path(&run.id)?, run).await
    }

    async fn load_run(&self, run_id: &str) -> Result<AgentRun, DomainError> {
        Self::read_json(&self.index_run_path(run_id)?).await
    }

    async fn save_run(&self, run: &AgentRun) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        Self::write_json_atomic(&run_dir.join("run.json"), run).await?;
        Self::write_json_atomic(&self.index_run_path(&run.id)?, run).await
    }

    async fn append_event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<AgentRunEvent, DomainError> {
        let _guard = self.event_lock.lock().await;
        let run_dir = self.load_run_dir(run_id).await?;
        let events_path = run_dir.join("events.jsonl");
        if let Some(parent) = events_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create agent event journal parent {}: {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        let seq = self
            .read_all_events(run_id)
            .await?
            .last()
            .map(|event| event.seq + 1)
            .unwrap_or(1);

        let event = AgentRunEvent {
            seq,
            id: format!("evt_{}", Uuid::new_v4().simple()),
            run_id: run_id.to_string(),
            timestamp: Utc::now(),
            level,
            event_type: event_type.to_string(),
            payload,
        };

        let line = serde_json::to_string(&event).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize agent event: {error}"))
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to open agent event journal {}: {}",
                    events_path.display(),
                    error
                ))
            })?;
        file.write_all(line.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to append agent event journal {}: {}",
                events_path.display(),
                error
            ))
        })?;
        file.write_all(b"\n").await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to append agent event journal newline {}: {}",
                events_path.display(),
                error
            ))
        })?;
        file.flush().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to flush agent event journal {}: {}",
                events_path.display(),
                error
            ))
        })?;

        Ok(event)
    }

    async fn read_events(
        &self,
        run_id: &str,
        query: AgentRunEventReadQuery,
    ) -> Result<Vec<AgentRunEvent>, DomainError> {
        let limit = query.limit.clamp(1, 500);
        let mut events = self.read_all_events(run_id).await?;

        if let Some(before_seq) = query.before_seq {
            events.retain(|event| event.seq < before_seq);
            let start = events.len().saturating_sub(limit);
            return Ok(events.into_iter().skip(start).collect());
        }

        if let Some(after_seq) = query.after_seq {
            events.retain(|event| event.seq > after_seq);
        }

        events.truncate(limit);
        Ok(events)
    }
}
