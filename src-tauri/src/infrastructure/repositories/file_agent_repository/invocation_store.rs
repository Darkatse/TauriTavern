use async_trait::async_trait;
use tokio::fs;

use super::FileAgentRepository;
use super::paths::validate_segment;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentInvocation, AgentTaskRecord};
use crate::domain::repositories::agent_invocation_repository::AgentInvocationRepository;

#[async_trait]
impl AgentInvocationRepository for FileAgentRepository {
    async fn save_invocation(&self, invocation: &AgentInvocation) -> Result<(), DomainError> {
        validate_segment(&invocation.run_id, "run_id")?;
        validate_segment(&invocation.id, "invocation_id")?;
        let directory = self
            .load_run_dir(&invocation.run_id)
            .await?
            .join("invocations");
        fs::create_dir_all(&directory).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create agent invocation directory {}: {}",
                directory.display(),
                error
            ))
        })?;
        Self::write_json_atomic(
            &directory.join(format!("{}.json", invocation.id)),
            invocation,
        )
        .await
    }

    async fn load_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<AgentInvocation, DomainError> {
        validate_segment(run_id, "run_id")?;
        validate_segment(invocation_id, "invocation_id")?;
        let path = self
            .load_run_dir(run_id)
            .await?
            .join("invocations")
            .join(format!("{invocation_id}.json"));
        Self::read_json(&path).await
    }

    async fn try_load_invocation(
        &self,
        run_id: &str,
        invocation_id: &str,
    ) -> Result<Option<AgentInvocation>, DomainError> {
        validate_segment(run_id, "run_id")?;
        validate_segment(invocation_id, "invocation_id")?;
        let path = self
            .load_run_dir(run_id)
            .await?
            .join("invocations")
            .join(format!("{invocation_id}.json"));
        Self::try_read_json(&path).await
    }

    async fn list_invocations(&self, run_id: &str) -> Result<Vec<AgentInvocation>, DomainError> {
        validate_segment(run_id, "run_id")?;
        let directory = self.load_run_dir(run_id).await?.join("invocations");
        let mut entries = match fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read agent invocation directory {}: {}",
                    directory.display(),
                    error
                )));
            }
        };

        let mut invocations = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent invocation directory entry {}: {}",
                directory.display(),
                error
            ))
        })? {
            if !entry
                .file_type()
                .await
                .map(|file_type| file_type.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            invocations.push(Self::read_json::<AgentInvocation>(&entry.path()).await?);
        }
        invocations.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(invocations)
    }

    async fn save_task(&self, task: &AgentTaskRecord) -> Result<(), DomainError> {
        validate_segment(&task.run_id, "run_id")?;
        validate_segment(&task.id, "task_id")?;
        let directory = self.load_run_dir(&task.run_id).await?.join("tasks");
        fs::create_dir_all(&directory).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create agent task directory {}: {}",
                directory.display(),
                error
            ))
        })?;
        Self::write_json_atomic(&directory.join(format!("{}.json", task.id)), task).await
    }

    async fn load_task(&self, run_id: &str, task_id: &str) -> Result<AgentTaskRecord, DomainError> {
        validate_segment(run_id, "run_id")?;
        validate_segment(task_id, "task_id")?;
        let path = self
            .load_run_dir(run_id)
            .await?
            .join("tasks")
            .join(format!("{task_id}.json"));
        Self::read_json(&path).await
    }

    async fn list_tasks(&self, run_id: &str) -> Result<Vec<AgentTaskRecord>, DomainError> {
        validate_segment(run_id, "run_id")?;
        let directory = self.load_run_dir(run_id).await?.join("tasks");
        let mut entries = match fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read agent task directory {}: {}",
                    directory.display(),
                    error
                )));
            }
        };

        let mut tasks = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent task directory entry {}: {}",
                directory.display(),
                error
            ))
        })? {
            if !entry
                .file_type()
                .await
                .map(|file_type| file_type.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            tasks.push(Self::read_json::<AgentTaskRecord>(&entry.path()).await?);
        }
        tasks.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(tasks)
    }
}
