use async_trait::async_trait;
use chrono::Utc;
use tokio::fs;

use super::FileAgentRepository;
use super::fs_tree::{sha256_hex, workspace_file_from_text};
use super::paths::validate_segment;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{Checkpoint, CheckpointFile, WorkspacePath};
use tt_ports::repositories::checkpoint_repository::CheckpointRepository;
use tt_ports::repositories::workspace_repository::WorkspaceFile;

#[async_trait]
impl CheckpointRepository for FileAgentRepository {
    async fn create_checkpoint(
        &self,
        run_id: &str,
        reason: &str,
        event_seq: u64,
        snapshot_files: &[WorkspaceFile],
    ) -> Result<Checkpoint, DomainError> {
        let _guard = self.checkpoint_lock.lock().await;
        let run_dir = self.load_run_dir(run_id).await?;
        let checkpoints_dir = run_dir.join("checkpoints");
        fs::create_dir_all(&checkpoints_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create checkpoint directory {}: {}",
                    checkpoints_dir.display(),
                    error
                ))
            })?;

        let mut next_seq = 1_u64;
        let mut entries = fs::read_dir(&checkpoints_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read checkpoint directory {}: {}",
                checkpoints_dir.display(),
                error
            ))
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read checkpoint directory entry {}: {}",
                checkpoints_dir.display(),
                error
            ))
        })? {
            if entry
                .file_type()
                .await
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
            {
                next_seq += 1;
            }
        }

        let checkpoint_id = format!("cp_{next_seq:06}");
        let checkpoint_dir = checkpoints_dir.join(&checkpoint_id);
        fs::create_dir_all(&checkpoint_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create checkpoint {}: {}",
                checkpoint_dir.display(),
                error
            ))
        })?;

        let mut files = Vec::new();
        for file in snapshot_files {
            let bytes = file.text.as_bytes();
            let target = checkpoint_dir.join(file.path.as_str());
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create checkpoint file parent {}: {}",
                        parent.display(),
                        error
                    ))
                })?;
            }
            fs::write(&target, &bytes).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write checkpoint file {}: {}",
                    target.display(),
                    error
                ))
            })?;

            files.push(CheckpointFile {
                path: file.path.as_str().to_string(),
                sha256: sha256_hex(bytes),
                bytes: bytes.len() as u64,
            });
        }

        let checkpoint = Checkpoint {
            id: checkpoint_id,
            seq: next_seq,
            run_id: run_id.to_string(),
            created_at: Utc::now(),
            reason: reason.to_string(),
            event_seq,
            files,
        };
        Self::write_json_atomic(&checkpoint_dir.join("checkpoint.json"), &checkpoint).await?;

        Ok(checkpoint)
    }

    async fn read_checkpoint_text(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        validate_segment(checkpoint_id, "checkpoint_id")?;
        let checkpoint_dir = self
            .load_run_dir(run_id)
            .await?
            .join("checkpoints")
            .join(checkpoint_id);
        let checkpoint: Checkpoint =
            Self::read_json(&checkpoint_dir.join("checkpoint.json")).await?;
        if checkpoint.run_id != run_id {
            return Err(DomainError::InvalidData(format!(
                "Checkpoint `{checkpoint_id}` belongs to another run"
            )));
        }
        let expected = checkpoint
            .files
            .iter()
            .find(|file| file.path == path.as_str())
            .ok_or_else(|| {
                DomainError::NotFound(format!(
                    "Checkpoint file not found: {checkpoint_id}/{}",
                    path.as_str()
                ))
            })?;
        let target = checkpoint_dir.join(path.as_str());
        let text = fs::read_to_string(&target).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "Checkpoint file not found: {checkpoint_id}/{}",
                    path.as_str()
                ))
            } else if error.kind() == std::io::ErrorKind::InvalidData {
                DomainError::workspace_file_not_text(path.as_str())
            } else {
                DomainError::InternalError(format!(
                    "Failed to read checkpoint file {}: {error}",
                    target.display()
                ))
            }
        })?;
        let file = workspace_file_from_text(path.clone(), text)?;
        if file.sha256 != expected.sha256 {
            return Err(DomainError::InvalidData(format!(
                "Checkpoint file hash mismatch: {checkpoint_id}/{}",
                path.as_str()
            )));
        }
        Ok(file)
    }
}
