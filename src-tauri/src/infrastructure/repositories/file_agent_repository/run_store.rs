use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::FileAgentRepository;
use super::run_record::{read_agent_run_record, write_agent_run_record};
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentRun, AgentRunEvent, AgentRunEventLevel, AgentRunSummaryProjection,
};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunListCursor, AgentRunListQuery, AgentRunRepository,
    AgentRunStorageStats, event_belongs_to_invocation,
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

        write_agent_run_record(&run_dir.join("run.json"), run).await?;
        write_agent_run_record(&self.index_run_path(&run.id)?, run).await
    }

    async fn load_run(&self, run_id: &str) -> Result<AgentRun, DomainError> {
        read_agent_run_record(&self.index_run_path(run_id)?).await
    }

    async fn list_runs(&self, query: AgentRunListQuery) -> Result<Vec<AgentRun>, DomainError> {
        let mut runs = self.read_indexed_runs().await?;
        runs.retain(|run| run_matches_list_query(run, &query));
        sort_runs_newest_first(&mut runs);
        runs.truncate(query.limit);
        Ok(runs)
    }

    async fn list_all_runs(&self) -> Result<Vec<AgentRun>, DomainError> {
        let mut runs = self.read_indexed_runs().await?;
        sort_runs_newest_first(&mut runs);
        Ok(runs)
    }

    async fn inspect_run_storage(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageStats, DomainError> {
        FileAgentRepository::inspect_run_storage(self, run).await
    }

    async fn load_run_summary_projection(
        &self,
        run_id: &str,
    ) -> Result<Option<AgentRunSummaryProjection>, DomainError> {
        let projection: Option<AgentRunSummaryProjection> =
            Self::try_read_json(&self.index_run_summary_path(run_id)?).await?;
        if let Some(projection) = projection.as_ref()
            && projection.run_id != run_id
        {
            return Err(DomainError::InvalidData(format!(
                "Agent run summary id mismatch for {}: found {}",
                run_id, projection.run_id
            )));
        }
        Ok(projection)
    }

    async fn save_run_summary_projection(
        &self,
        projection: &AgentRunSummaryProjection,
    ) -> Result<(), DomainError> {
        Self::write_json_atomic(
            &self.index_run_summary_path(&projection.run_id)?,
            projection,
        )
        .await
    }

    async fn save_run(&self, run: &AgentRun) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        write_agent_run_record(&run_dir.join("run.json"), run).await?;
        write_agent_run_record(&self.index_run_path(&run.id)?, run).await
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

        if let Some(invocation_id) = query.invocation_id.as_deref() {
            events.retain(|event| event_belongs_to_invocation(event, invocation_id));
        }

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

    async fn read_all_events(&self, run_id: &str) -> Result<Vec<AgentRunEvent>, DomainError> {
        FileAgentRepository::read_all_events(self, run_id).await
    }
}

impl FileAgentRepository {
    async fn read_indexed_runs(&self) -> Result<Vec<AgentRun>, DomainError> {
        let index_dir = self.root.join("index").join("runs");
        let metadata = match fs::symlink_metadata(&index_dir).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect agent run index {}: {}",
                    index_dir.display(),
                    error
                )));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Agent run index is not a directory: {}",
                index_dir.display()
            )));
        }

        let mut entries = fs::read_dir(&index_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent run index {}: {}",
                index_dir.display(),
                error
            ))
        })?;
        let mut runs = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to scan agent run index {}: {}",
                index_dir.display(),
                error
            ))
        })? {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let metadata = fs::symlink_metadata(&path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect agent run index entry {}: {}",
                    path.display(),
                    error
                ))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(DomainError::InvalidData(format!(
                    "Agent run index entry is not a file: {}",
                    path.display()
                )));
            }

            let run_id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    DomainError::InvalidData(format!(
                        "Invalid agent run index file name: {}",
                        path.display()
                    ))
                })?;
            super::paths::validate_segment(run_id, "run_id")?;

            let run = read_agent_run_record(&path).await?;
            if run.id != run_id {
                return Err(DomainError::InvalidData(format!(
                    "Agent run index id mismatch in {}: expected {}, found {}",
                    path.display(),
                    run_id,
                    run.id
                )));
            }
            runs.push(run);
        }

        Ok(runs)
    }
}

fn run_matches_list_query(run: &AgentRun, query: &AgentRunListQuery) -> bool {
    if query
        .chat_ref
        .as_ref()
        .is_some_and(|chat_ref| &run.chat_ref != chat_ref)
    {
        return false;
    }
    if query
        .stable_chat_id
        .as_ref()
        .is_some_and(|stable_chat_id| &run.stable_chat_id != stable_chat_id)
    {
        return false;
    }
    if query
        .statuses
        .as_ref()
        .is_some_and(|statuses| !statuses.contains(&run.status))
    {
        return false;
    }
    if query
        .before
        .as_ref()
        .is_some_and(|cursor| !run_is_before_cursor(run, cursor))
    {
        return false;
    }
    true
}

fn sort_runs_newest_first(runs: &mut [AgentRun]) {
    runs.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}

fn run_is_before_cursor(run: &AgentRun, cursor: &AgentRunListCursor) -> bool {
    run.created_at < cursor.created_at
        || (run.created_at == cursor.created_at && run.id.as_str() < cursor.run_id.as_str())
}
