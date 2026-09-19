use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncReadExt;

use super::persistent_store::{PersistentSnapshotFile, PersistentTree};
use crate::hashing::hex_lower;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;

pub(super) fn should_skip_platform_metadata_file(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<bool, DomainError> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    if file_name != ".DS_Store" {
        return Ok(false);
    }
    if metadata.is_file() {
        return Ok(true);
    }
    Err(DomainError::InvalidData(format!(
        "Platform metadata entry is not a file: {}",
        path.display()
    )))
}

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
            if should_skip_platform_metadata_file(&child, &metadata)? {
                continue;
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
                fs::copy(&child, &target_child).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to copy persistent file {} to {}: {}",
                        child.display(),
                        target_child.display(),
                        error
                    ))
                })?;
            } else {
                return Err(DomainError::InvalidData(format!(
                    "Unsupported persistent node: {}",
                    child.display()
                )));
            }
        }
    }

    Ok(())
}

pub(super) async fn scan_workspace_tree(
    root: &Path,
    root_path: &str,
    include_files: bool,
) -> Result<PersistentTree, DomainError> {
    let root_metadata = match fs::symlink_metadata(root).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DomainError::NotFound(format!(
                "Persistent root missing: {}",
                root.display()
            )));
        }
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

    let mut tree = PersistentTree::default();
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
            if should_skip_platform_metadata_file(&child, &metadata)? {
                continue;
            }
            let path = logical_workspace_path(root, root_path, &child)?;
            if metadata.is_dir() {
                tree.directories.push(path);
                stack.push(child);
            } else if metadata.is_file() {
                if include_files {
                    let (sha256, bytes) = hash_file(&child).await?;
                    tree.files.push(PersistentSnapshotFile {
                        path,
                        sha256,
                        bytes,
                    });
                }
            } else {
                return Err(DomainError::InvalidData(format!(
                    "Unsupported persistent node: {}",
                    child.display()
                )));
            }
        }
    }

    tree.files.sort_by(|a, b| a.path.cmp(&b.path));
    tree.directories.sort();
    Ok(tree)
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
        .map(|part| {
            part.to_str().ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Persistent path is not UTF-8: {}",
                    target.display()
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    let value = if suffix.is_empty() {
        root_path.to_string()
    } else {
        format!("{root_path}/{suffix}")
    };
    Ok(WorkspacePath::parse(value)?.as_str().to_string())
}

pub(super) async fn hash_file(path: &Path) -> Result<(String, u64), DomainError> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|e| DomainError::file_io("hash", path.display().to_string(), e))?;
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|e| DomainError::file_io("hash", path.display().to_string(), e))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((hex_lower(&hash.finalize()), bytes))
}
