use std::path::PathBuf;

use tokio::fs;

use super::FileAgentRepository;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentRun, WorkspacePath};
use crate::domain::repositories::agent_run_repository::AgentRunRepository;

impl FileAgentRepository {
    pub(super) fn index_run_path(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        validate_segment(run_id, "run_id")?;
        Ok(self
            .root
            .join("index")
            .join("runs")
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

    pub(super) async fn load_run_dir(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        let run = self.load_run(run_id).await?;
        self.run_dir(&run)
    }

    pub(super) async fn safe_workspace_path(
        &self,
        run_id: &str,
        workspace_path: &WorkspacePath,
        create_parent: bool,
    ) -> Result<PathBuf, DomainError> {
        let run_dir = self.load_run_dir(run_id).await?;
        let target = run_dir.join(workspace_path.as_str());

        let canonical_run_dir = fs::canonicalize(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve agent workspace root {}: {}",
                run_dir.display(),
                error
            ))
        })?;

        if let Some(parent) = target.parent() {
            if create_parent {
                fs::create_dir_all(parent).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create workspace parent {}: {}",
                        parent.display(),
                        error
                    ))
                })?;
            }

            let canonical_parent = fs::canonicalize(parent).await.map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    DomainError::NotFound(format!(
                        "Workspace path parent not found: {}",
                        workspace_path.as_str()
                    ))
                } else {
                    DomainError::InternalError(format!(
                        "Failed to resolve workspace parent {}: {}",
                        parent.display(),
                        error
                    ))
                }
            })?;
            if !canonical_parent.starts_with(&canonical_run_dir) {
                return Err(DomainError::InvalidData(format!(
                    "Workspace path escapes run directory: {}",
                    workspace_path.as_str()
                )));
            }
        }

        match fs::symlink_metadata(&target).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DomainError::InvalidData(format!(
                    "Workspace path targets a symlink: {}",
                    workspace_path.as_str()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect workspace path {}: {}",
                    target.display(),
                    error
                )));
            }
        }

        Ok(target)
    }

    pub(super) async fn safe_chat_workspace_path(
        &self,
        workspace_id: &str,
        workspace_path: &WorkspacePath,
        create_parent: bool,
    ) -> Result<PathBuf, DomainError> {
        let chat_dir = self.chat_dir(workspace_id)?;
        fs::create_dir_all(&chat_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create agent chat workspace {}: {}",
                chat_dir.display(),
                error
            ))
        })?;
        let target = chat_dir.join(workspace_path.as_str());

        let canonical_chat_dir = fs::canonicalize(&chat_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve agent chat workspace {}: {}",
                chat_dir.display(),
                error
            ))
        })?;

        if let Some(parent) = target.parent() {
            if create_parent {
                fs::create_dir_all(parent).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create persistent workspace parent {}: {}",
                        parent.display(),
                        error
                    ))
                })?;
            }

            let canonical_parent = fs::canonicalize(parent).await.map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    DomainError::NotFound(format!(
                        "Persistent workspace path parent not found: {}",
                        workspace_path.as_str()
                    ))
                } else {
                    DomainError::InternalError(format!(
                        "Failed to resolve persistent workspace parent {}: {}",
                        parent.display(),
                        error
                    ))
                }
            })?;
            if !canonical_parent.starts_with(&canonical_chat_dir) {
                return Err(DomainError::InvalidData(format!(
                    "Persistent workspace path escapes chat workspace: {}",
                    workspace_path.as_str()
                )));
            }
        }

        match fs::symlink_metadata(&target).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DomainError::InvalidData(format!(
                    "Persistent workspace path targets a symlink: {}",
                    workspace_path.as_str()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect persistent workspace path {}: {}",
                    target.display(),
                    error
                )));
            }
        }

        Ok(target)
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
            | "tool-results"
            | "checkpoints"
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
