use std::path::Path;

use tokio::fs;

use super::FileAgentRepository;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::{AgentRun, storage::AgentRunStorageClass};
use crate::domain::repositories::agent_run_repository::{
    AgentRunStorageEntryStats, AgentRunStorageStats,
};

impl FileAgentRepository {
    pub(super) async fn inspect_run_storage(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageStats, DomainError> {
        let run_dir = self.run_dir(run)?;
        let mut stats = AgentRunStorageStats::default();

        inspect_run_dir(&run_dir, &mut stats).await?;
        add_required_index_file(&self.index_run_path(&run.id)?, &mut stats).await?;
        add_optional_index_file(&self.index_run_summary_path(&run.id)?, &mut stats).await?;

        Ok(stats)
    }
}

async fn inspect_run_dir(
    run_dir: &Path,
    stats: &mut AgentRunStorageStats,
) -> Result<(), DomainError> {
    let metadata = fs::symlink_metadata(run_dir).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::InvalidData(format!(
                "Agent run workspace is missing: {}",
                run_dir.display()
            ))
        } else {
            DomainError::InternalError(format!(
                "Failed to inspect agent run workspace {}: {}",
                run_dir.display(),
                error
            ))
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Agent run workspace is not a directory: {}",
            run_dir.display()
        )));
    }

    let mut pending = vec![(run_dir.to_path_buf(), String::new())];
    while let Some((dir, relative_dir)) = pending.pop() {
        let mut entries = fs::read_dir(&dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent run storage directory {}: {}",
                dir.display(),
                error
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent run storage entry {}: {}",
                dir.display(),
                error
            ))
        })? {
            let path = entry.path();
            let name = entry.file_name().into_string().map_err(|_| {
                DomainError::InvalidData(format!(
                    "Agent run storage entry is not UTF-8: {}",
                    path.display()
                ))
            })?;
            let relative_path = if relative_dir.is_empty() {
                name
            } else {
                format!("{relative_dir}/{name}")
            };

            let metadata = fs::symlink_metadata(&path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect agent run storage entry {}: {}",
                    path.display(),
                    error
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(DomainError::InvalidData(format!(
                    "Agent run storage entry is a symlink: {}",
                    path.display()
                )));
            }
            if metadata.is_dir() {
                pending.push((path, relative_path));
                continue;
            }
            if !metadata.is_file() {
                return Err(DomainError::InvalidData(format!(
                    "Agent run storage entry is not a file or directory: {}",
                    path.display()
                )));
            }

            let storage_class = AgentRunStorageClass::from_run_relative_path(&relative_path);
            add_file_for_class(stats, storage_class, metadata.len(), &path)?;
        }
    }

    Ok(())
}

async fn add_required_index_file(
    path: &Path,
    stats: &mut AgentRunStorageStats,
) -> Result<(), DomainError> {
    let metadata = fs::symlink_metadata(path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            DomainError::InvalidData(format!(
                "Agent run index file is missing: {}",
                path.display()
            ))
        } else {
            DomainError::InternalError(format!(
                "Failed to inspect agent run index file {}: {}",
                path.display(),
                error
            ))
        }
    })?;
    add_index_file_metadata(path, metadata, AgentRunStorageClass::run_index(), stats)
}

async fn add_optional_index_file(
    path: &Path,
    stats: &mut AgentRunStorageStats,
) -> Result<(), DomainError> {
    let metadata = match fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to inspect agent run index file {}: {}",
                path.display(),
                error
            )));
        }
    };
    add_index_file_metadata(
        path,
        metadata,
        AgentRunStorageClass::run_summary_projection(),
        stats,
    )
}

fn add_index_file_metadata(
    path: &Path,
    metadata: std::fs::Metadata,
    storage_class: AgentRunStorageClass,
    stats: &mut AgentRunStorageStats,
) -> Result<(), DomainError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Agent run index path is not a file: {}",
            path.display()
        )));
    }
    add_file_for_class(stats, storage_class, metadata.len(), path)
}

fn add_file_for_class(
    stats: &mut AgentRunStorageStats,
    storage_class: AgentRunStorageClass,
    bytes: u64,
    path: &Path,
) -> Result<(), DomainError> {
    add_file(&mut stats.total, bytes, path)?;
    if storage_class.is_slim_artifact() {
        add_file(&mut stats.heavy_artifacts, bytes, path)?;
    }
    Ok(())
}

fn add_file(
    stats: &mut AgentRunStorageEntryStats,
    bytes: u64,
    path: &Path,
) -> Result<(), DomainError> {
    stats.file_count = stats.file_count.checked_add(1).ok_or_else(|| {
        DomainError::InternalError(format!(
            "Agent run storage file count overflow at {}",
            path.display()
        ))
    })?;
    stats.byte_count = stats.byte_count.checked_add(bytes).ok_or_else(|| {
        DomainError::InternalError(format!(
            "Agent run storage byte count overflow at {}",
            path.display()
        ))
    })?;
    Ok(())
}
