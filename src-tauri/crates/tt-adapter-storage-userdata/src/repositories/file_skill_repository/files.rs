use std::path::PathBuf;

use tokio::fs;
use tokio::io::AsyncReadExt;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;
use tt_domain::models::skill::SkillScope;
use tt_ports::workspace_fs::{WorkspaceDirectoryEntry, WorkspaceEntryKind, WorkspaceMetadata};

use super::paths::normalize_skill_path;
use super::{FileSkillRepository, MAX_SINGLE_FILE_BYTES};

pub(super) async fn read_bytes(
    repository: &FileSkillRepository,
    scope: &SkillScope,
    name: &str,
    path: &WorkspacePath,
    maximum_bytes: usize,
) -> Result<Vec<u8>, DomainError> {
    let label = logical_path(name, Some(path));
    let (target, metadata) = resolve(repository, scope, name, Some(path)).await?;
    if metadata.kind == WorkspaceEntryKind::Directory {
        return Err(DomainError::workspace_path_is_directory(label));
    }
    let maximum_bytes = maximum_bytes.min(MAX_SINGLE_FILE_BYTES as usize);
    let file = fs::File::open(target)
        .await
        .map_err(|error| DomainError::file_io("read", &label, error))?;
    let mut bytes = Vec::new();
    file.take((maximum_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| DomainError::file_io("read", &label, error))?;
    if bytes.len() > maximum_bytes {
        return Err(DomainError::InvalidData(format!(
            "Skill read exceeds {maximum_bytes} bytes: {label}"
        )));
    }
    Ok(bytes)
}

pub(super) async fn metadata(
    repository: &FileSkillRepository,
    scope: &SkillScope,
    name: &str,
    path: Option<&WorkspacePath>,
) -> Result<WorkspaceMetadata, DomainError> {
    Ok(resolve(repository, scope, name, path).await?.1)
}

pub(super) async fn read_dir(
    repository: &FileSkillRepository,
    scope: &SkillScope,
    name: &str,
    path: Option<&WorkspacePath>,
    maximum_entries: usize,
) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
    let label = logical_path(name, path);
    let (target, _) = resolve(repository, scope, name, path).await?;
    let mut reader = fs::read_dir(target)
        .await
        .map_err(|error| DomainError::file_io("list", &label, error))?;
    let mut entries = Vec::new();
    while let Some(entry) = reader
        .next_entry()
        .await
        .map_err(|error| DomainError::file_io("list", &label, error))?
    {
        if entries.len() == maximum_entries {
            return Err(DomainError::InvalidData(format!(
                "Skill directory exceeds {maximum_entries} entries: {label}"
            )));
        }
        let filename = entry.file_name().into_string().map_err(|_| {
            DomainError::InvalidData(format!("Skill filename is not UTF-8: {label}"))
        })?;
        let child = match path {
            Some(path) => format!("{}/{filename}", path.as_str()),
            None => filename,
        };
        let child = WorkspacePath::parse(normalize_skill_path(&child)?)?;
        let child_label = logical_path(name, Some(&child));
        let metadata = fs::symlink_metadata(entry.path())
            .await
            .map_err(|error| DomainError::file_io("stat", &child_label, error))?;
        entries.push(WorkspaceDirectoryEntry {
            path: child,
            metadata: node_metadata(metadata, &child_label)?,
        });
    }
    entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    Ok(entries)
}

async fn resolve(
    repository: &FileSkillRepository,
    scope: &SkillScope,
    name: &str,
    path: Option<&WorkspacePath>,
) -> Result<(PathBuf, WorkspaceMetadata), DomainError> {
    let root = repository.installed_skill_root(scope, name).await?;
    let label = logical_path(name, path);
    let target = match path {
        Some(path) => root.join(normalize_skill_path(path.as_str())?),
        None => root.clone(),
    };
    let metadata = fs::symlink_metadata(&target)
        .await
        .map_err(|error| DomainError::file_io("stat", &label, error))?;
    let metadata = node_metadata(metadata, &label)?;
    let canonical_root = fs::canonicalize(root)
        .await
        .map_err(|error| DomainError::file_io("resolve", format!("skills/{name}"), error))?;
    let canonical_target = fs::canonicalize(&target)
        .await
        .map_err(|error| DomainError::file_io("resolve", &label, error))?;
    if !canonical_target.starts_with(canonical_root) {
        return Err(DomainError::InvalidData(format!(
            "Skill path escapes installed directory: {label}"
        )));
    }
    Ok((target, metadata))
}

fn logical_path(name: &str, path: Option<&WorkspacePath>) -> String {
    match path {
        Some(path) => format!("skills/{name}/{}", path.as_str()),
        None => format!("skills/{name}"),
    }
}

fn node_metadata(
    metadata: std::fs::Metadata,
    path: &str,
) -> Result<WorkspaceMetadata, DomainError> {
    let kind = if metadata.is_file() {
        WorkspaceEntryKind::File
    } else if metadata.is_dir() {
        WorkspaceEntryKind::Directory
    } else {
        return Err(DomainError::InvalidData(format!(
            "Skill path must be a regular file or directory, not a symlink or special file: {path}"
        )));
    };
    Ok(WorkspaceMetadata {
        kind,
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
    })
}
