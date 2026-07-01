use std::path::{Path, PathBuf};

use tokio::fs;

use super::FileAgentRepository;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentRun, storage::AgentRunStorageClass};
use tt_ports::repositories::agent_run_repository::{
    AgentRunStorageEntryStats, AgentRunStorageStats,
};

impl FileAgentRepository {
    pub(super) async fn inspect_run_storage(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageStats, DomainError> {
        let run_dir = self.run_dir(run)?;
        let mut stats = AgentRunStorageStats::default();

        add_run_scan_stats(scan_run_storage(&run_dir).await?, &mut stats)?;
        add_required_index_file(&self.index_run_path(&run.id)?, &mut stats).await?;
        add_optional_index_file(&self.index_run_summary_path(&run.id)?, &mut stats).await?;

        Ok(stats)
    }

    pub(super) async fn slim_run_heavy_artifacts(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageEntryStats, DomainError> {
        let run_dir = self.run_dir(run)?;
        let scan = scan_run_storage(&run_dir).await?;
        let mut removed = AgentRunStorageEntryStats::default();

        for file in scan.files {
            if !file.storage_class.is_slim_artifact() {
                continue;
            }
            fs::remove_file(&file.path).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to delete agent run artifact {}: {}",
                    file.path.display(),
                    error
                ))
            })?;
            add_file(&mut removed, file.bytes, &file.path)?;
        }

        remove_empty_run_dirs(&run_dir, scan.dirs).await?;
        Ok(removed)
    }

    pub(super) async fn delete_run(
        &self,
        run: &AgentRun,
    ) -> Result<AgentRunStorageEntryStats, DomainError> {
        let stats = self.inspect_run_storage(run).await?.total;
        let run_dir = self.run_dir(run)?;

        fs::remove_dir_all(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete agent run workspace {}: {}",
                run_dir.display(),
                error
            ))
        })?;

        remove_index_file_if_exists(&self.index_run_path(&run.id)?, "agent run index").await?;
        remove_index_file_if_exists(&self.index_run_summary_path(&run.id)?, "agent run summary")
            .await?;

        Ok(stats)
    }
}

struct AgentRunStorageScan {
    files: Vec<AgentRunStorageFile>,
    dirs: Vec<PathBuf>,
}

struct AgentRunStorageFile {
    path: PathBuf,
    storage_class: AgentRunStorageClass,
    bytes: u64,
}

async fn scan_run_storage(run_dir: &Path) -> Result<AgentRunStorageScan, DomainError> {
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

    let mut files = Vec::new();
    let mut dirs = Vec::new();
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
            files.push(AgentRunStorageFile {
                path,
                storage_class,
                bytes: metadata.len(),
            });
        }

        if dir != run_dir {
            dirs.push(dir);
        }
    }

    Ok(AgentRunStorageScan { files, dirs })
}

fn add_run_scan_stats(
    scan: AgentRunStorageScan,
    stats: &mut AgentRunStorageStats,
) -> Result<(), DomainError> {
    for file in scan.files {
        add_file_for_class(stats, file.storage_class, file.bytes, &file.path)?;
    }
    Ok(())
}

async fn remove_empty_run_dirs(run_dir: &Path, mut dirs: Vec<PathBuf>) -> Result<(), DomainError> {
    dirs.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for dir in dirs {
        if dir == run_dir {
            continue;
        }
        match fs::remove_dir(&dir).await {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to remove empty agent run artifact directory {}: {}",
                    dir.display(),
                    error
                )));
            }
        }
    }
    Ok(())
}

async fn remove_index_file_if_exists(path: &Path, label: &str) -> Result<(), DomainError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to delete {label} {}: {}",
            path.display(),
            error
        ))),
    }
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
