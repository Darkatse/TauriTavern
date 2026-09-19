use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::FileAgentRepository;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentRun, WorkspacePath};
use tt_ports::repositories::agent_run_repository::AgentRunRepository;

impl FileAgentRepository {
    pub(super) fn index_run_path(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        validate_segment(run_id, "run_id")?;
        Ok(self
            .root
            .join("index")
            .join("runs")
            .join(format!("{run_id}.json")))
    }

    pub(super) fn index_run_summary_path(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        validate_segment(run_id, "run_id")?;
        Ok(self
            .root
            .join("index")
            .join("run-summaries")
            .join(format!("{run_id}.json")))
    }

    pub(super) fn run_dir(&self, run: &AgentRun) -> Result<PathBuf, DomainError> {
        validate_segment(&run.workspace_id, "workspace_id")?;
        validate_segment(&run.id, "run_id")?;
        Ok(self
            .root
            .join("chats")
            .join(&run.workspace_id)
            .join("runs")
            .join(&run.id))
    }

    pub(super) fn chat_dir(&self, workspace_id: &str) -> Result<PathBuf, DomainError> {
        validate_segment(workspace_id, "workspace_id")?;
        Ok(self.root.join("chats").join(workspace_id))
    }

    pub(super) fn persistent_state_dir(
        &self,
        workspace_id: &str,
        state_id: &str,
    ) -> Result<PathBuf, DomainError> {
        validate_segment(workspace_id, "workspace_id")?;
        validate_segment(state_id, "persist_state_id")?;
        Ok(self
            .root
            .join("chats")
            .join(workspace_id)
            .join("persistent-states")
            .join(state_id))
    }

    pub(super) async fn load_run_dir(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        let run = self.load_run(run_id).await?;
        self.run_dir(&run)
    }

    pub(super) async fn workspace_lock(&self, run_id: &str) -> Arc<RwLock<()>> {
        let mut locks = self.workspace_locks.lock().await;
        if locks.len() > 4096 {
            locks.retain(|_, lock| lock.strong_count() > 0);
        }
        if let Some(lock) = locks.get(run_id).and_then(|lock| lock.upgrade()) {
            return lock;
        }
        // ponytail: serialize Run mutations; split only if measured contention warrants it.
        let lock = Arc::new(RwLock::new(()));
        locks.insert(run_id.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

pub(super) fn validate_workspace_root_path(value: &str) -> Result<String, DomainError> {
    let path = WorkspacePath::parse(value)?;
    if path.as_str().contains('/') {
        return Err(DomainError::InvalidData(format!(
            "Workspace root must be a single path segment: {}",
            path.as_str()
        )));
    }
    if matches!(
        path.as_str(),
        "runs"
            | "input"
            | "model-responses"
            | "tool-args"
            | "events.jsonl"
            | "manifest.json"
            | "run.json"
    ) {
        return Err(DomainError::InvalidData(format!(
            "Workspace root uses a reserved agent storage name: {}",
            path.as_str()
        )));
    }
    Ok(path.as_str().to_string())
}

pub(super) fn validate_segment(value: &str, label: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(DomainError::InvalidData(format!(
            "Invalid agent storage segment {label}: {value}"
        )));
    }
    Ok(())
}
