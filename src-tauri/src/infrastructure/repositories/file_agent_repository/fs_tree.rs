use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::fs;

use super::persistent_store::PersistentSnapshotFile;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::WorkspacePath;
use crate::domain::repositories::workspace_repository::WorkspaceFile;

pub(super) async fn copy_directory_contents(
    source: &Path,
    target: &Path,
) -> Result<(), DomainError> {
    let source_metadata = match fs::symlink_metadata(source).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to inspect persistent root {}: {}",
                source.display(),
                error
            )));
        }
    };
    if source_metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Persistent root targets a symlink: {}",
            source.display()
        )));
    }
    if !source_metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Persistent root is not a directory: {}",
            source.display()
        )));
    }

    let mut stack = vec![(source.to_path_buf(), target.to_path_buf())];
    while let Some((source_dir, target_dir)) = stack.pop() {
        fs::create_dir_all(&target_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create persistent projection directory {}: {}",
                target_dir.display(),
                error
            ))
        })?;

        let mut children = fs::read_dir(&source_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read persistent root {}: {}",
                source_dir.display(),
                error
            ))
        })?;
        let mut child_paths = Vec::new();
        while let Some(entry) = children.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read persistent root entry {}: {}",
                source_dir.display(),
                error
            ))
        })? {
            child_paths.push(entry.path());
        }
        child_paths.sort();

        for child in child_paths {
            let metadata = fs::symlink_metadata(&child).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect persistent root entry {}: {}",
                    child.display(),
                    error
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(DomainError::InvalidData(format!(
                    "Persistent root entry targets a symlink: {}",
                    child.display()
                )));
            }
            let relative = child.strip_prefix(&source_dir).map_err(|error| {
                DomainError::InvalidData(format!(
                    "Persistent root entry escaped scan root {}: {}",
                    source_dir.display(),
                    error
                ))
            })?;
            let target_child = target_dir.join(relative);
            if metadata.is_dir() {
                stack.push((child, target_child));
            } else if metadata.is_file() {
                let bytes = fs::read(&child).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to read persistent file {}: {}",
                        child.display(),
                        error
                    ))
                })?;
                if let Some(parent) = target_child.parent() {
                    fs::create_dir_all(parent).await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to create persistent projection file parent {}: {}",
                            parent.display(),
                            error
                        ))
                    })?;
                }
                fs::write(&target_child, bytes).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to write persistent projection file {}: {}",
                        target_child.display(),
                        error
                    ))
                })?;
            }
        }
    }

    Ok(())
}

pub(super) async fn scan_workspace_files(
    root: &Path,
    root_path: &str,
) -> Result<Vec<PersistentSnapshotFile>, DomainError> {
    let root_metadata = match fs::symlink_metadata(root).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(DomainError::InternalError(format!(
                "Failed to inspect workspace root {}: {}",
                root.display(),
                error
            )));
        }
    };
    if root_metadata.file_type().is_symlink() {
        return Err(DomainError::InvalidData(format!(
            "Workspace root targets a symlink: {}",
            root.display()
        )));
    }
    if !root_metadata.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "Workspace root is not a directory: {}",
            root.display()
        )));
    }

    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut children = fs::read_dir(&dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read workspace root {}: {}",
                dir.display(),
                error
            ))
        })?;
        let mut child_paths = Vec::new();
        while let Some(entry) = children.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read workspace root entry {}: {}",
                dir.display(),
                error
            ))
        })? {
            child_paths.push(entry.path());
        }
        child_paths.sort();

        for child in child_paths.into_iter().rev() {
            let metadata = fs::symlink_metadata(&child).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect workspace root entry {}: {}",
                    child.display(),
                    error
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(DomainError::InvalidData(format!(
                    "Workspace root entry targets a symlink: {}",
                    child.display()
                )));
            }
            if metadata.is_dir() {
                stack.push(child);
            } else if metadata.is_file() {
                let bytes = fs::read(&child).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to read workspace file {}: {}",
                        child.display(),
                        error
                    ))
                })?;
                files.push(PersistentSnapshotFile {
                    path: logical_workspace_path(root, root_path, &child)?,
                    sha256: sha256_hex(&bytes),
                    bytes: bytes.len() as u64,
                });
            }
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub(super) fn logical_workspace_path(
    root: &Path,
    root_path: &str,
    target: &Path,
) -> Result<String, DomainError> {
    let relative = target.strip_prefix(root).map_err(|error| {
        DomainError::InvalidData(format!(
            "Workspace path is outside root {}: {}",
            root.display(),
            error
        ))
    })?;
    let suffix = relative
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let value = if suffix.is_empty() {
        root_path.to_string()
    } else {
        format!("{root_path}/{suffix}")
    };
    Ok(WorkspacePath::parse(value)?.as_str().to_string())
}

pub(super) fn snapshot_map(
    files: Vec<PersistentSnapshotFile>,
) -> BTreeMap<String, PersistentSnapshotFile> {
    files
        .into_iter()
        .map(|file| (file.path.clone(), file))
        .collect()
}

pub(super) fn workspace_file_from_text(
    path: WorkspacePath,
    text: String,
) -> Result<WorkspaceFile, DomainError> {
    let bytes = text.as_bytes().to_vec();
    Ok(WorkspaceFile {
        path,
        text,
        bytes: bytes.len() as u64,
        sha256: sha256_hex(&bytes),
    })
}

pub(super) fn workspace_path_from_run_dir(
    run_dir: &Path,
    target: &Path,
) -> Result<WorkspacePath, DomainError> {
    let relative = target.strip_prefix(run_dir).map_err(|error| {
        DomainError::InvalidData(format!(
            "Workspace path is outside run directory {}: {}",
            run_dir.display(),
            error
        ))
    })?;
    let value = relative
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    WorkspacePath::parse(value)
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
