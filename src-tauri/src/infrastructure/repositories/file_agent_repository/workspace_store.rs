use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;

use super::FileAgentRepository;
use super::fs_tree::{sha256_hex, workspace_file_from_text, workspace_path_from_run_dir};
use super::paths::validate_workspace_root_path;
use crate::domain::errors::{DomainError, WorkspaceWriteConflictKind};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentRun, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
};
use crate::domain::repositories::workspace_repository::{
    WorkspaceAppendResult, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFile, WorkspaceFileList,
    WorkspaceRepository, WorkspaceWriteGuard,
};
use crate::infrastructure::persistence::file_system::{
    replace_file_with_fallback, unique_temp_path,
};

#[async_trait]
impl WorkspaceRepository for FileAgentRepository {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        fs::create_dir_all(run_dir.join("input"))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create agent run input directory {}: {}",
                    run_dir.join("input").display(),
                    error
                ))
            })?;

        for root in &manifest.roots {
            let root_path = validate_workspace_root_path(&root.path)?;
            fs::create_dir_all(run_dir.join(root_path.as_str()))
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create agent workspace root {}: {}",
                        run_dir.join(root_path.as_str()).display(),
                        error
                    ))
                })?;
        }

        let persistent_snapshot = self
            .initialize_projected_roots(run, manifest, &run_dir)
            .await?;

        Self::write_json_atomic(&run_dir.join("manifest.json"), manifest).await?;
        Self::write_json_atomic(
            &run_dir.join("input").join("prompt_snapshot.json"),
            prompt_snapshot,
        )
        .await?;
        Self::write_json_atomic(
            &run_dir.join("input").join("resolved_profile.json"),
            resolved_profile,
        )
        .await?;
        Self::write_json_atomic(
            &run_dir.join("input").join("persist_snapshot.json"),
            &persistent_snapshot,
        )
        .await
    }

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        Self::read_json(&self.load_run_dir(run_id).await?.join("manifest.json")).await
    }

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError> {
        self.write_text_guarded(run_id, path, text, WorkspaceWriteGuard::Unchecked)
            .await
    }

    async fn write_text_guarded(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError> {
        let target = self.safe_workspace_path(run_id, path, true).await?;
        let _guard = self.acquire_workspace_write_lock(&target).await;
        ensure_target_is_not_directory(&target, path).await?;
        verify_workspace_write_guard(&target, path, guard).await?;
        let temp_path = unique_temp_path(&target, "workspace.txt");
        fs::write(&temp_path, text.as_bytes())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write workspace temp file {}: {}",
                    temp_path.display(),
                    error
                ))
            })?;
        replace_file_with_fallback(&temp_path, &target).await?;

        workspace_file_from_text(path.clone(), text.to_string())
    }

    async fn append_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        let target = self.safe_workspace_path(run_id, path, true).await?;
        let _guard = self.acquire_workspace_write_lock(&target).await;
        ensure_target_is_not_directory(&target, path).await?;
        let existing = read_existing_workspace_text(&target).await?;
        let previous_sha256 = existing.as_ref().map(|text| sha256_hex(text.as_bytes()));
        let updated = match existing {
            Some(mut existing) => {
                existing.push_str(text);
                existing
            }
            None => text.to_string(),
        };
        let temp_path = unique_temp_path(&target, "workspace.txt");
        fs::write(&temp_path, updated.as_bytes())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write workspace temp file {}: {}",
                    temp_path.display(),
                    error
                ))
            })?;
        replace_file_with_fallback(&temp_path, &target).await?;

        Ok(WorkspaceAppendResult {
            file: workspace_file_from_text(path.clone(), updated)?,
            previous_sha256,
        })
    }

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        let target = self.safe_workspace_path(run_id, path, false).await?;
        ensure_target_is_not_directory(&target, path).await?;
        let text = fs::read_to_string(&target).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("Workspace file not found: {}", path.as_str()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read workspace file {}: {}",
                    target.display(),
                    error
                ))
            }
        })?;

        workspace_file_from_text(path.clone(), text)
    }

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        let run_dir = self.load_run_dir(run_id).await?;
        let root = match path {
            Some(path) => self.safe_workspace_path(run_id, path, false).await?,
            None => run_dir.clone(),
        };

        let canonical_run_dir = fs::canonicalize(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve agent workspace root {}: {}",
                run_dir.display(),
                error
            ))
        })?;
        let canonical_root = fs::canonicalize(&root).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "Workspace path not found: {}",
                    path.map(WorkspacePath::as_str).unwrap_or(".")
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to resolve workspace list root {}: {}",
                    root.display(),
                    error
                ))
            }
        })?;
        if !canonical_root.starts_with(&canonical_run_dir) {
            return Err(DomainError::InvalidData(format!(
                "Workspace path escapes run directory: {}",
                path.map(WorkspacePath::as_str).unwrap_or(".")
            )));
        }

        let root_metadata = fs::symlink_metadata(&root).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "Workspace path not found: {}",
                    path.map(WorkspacePath::as_str).unwrap_or(".")
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to inspect workspace path {}: {}",
                    root.display(),
                    error
                ))
            }
        })?;
        if root_metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Workspace path targets a symlink: {}",
                path.map(WorkspacePath::as_str).unwrap_or(".")
            )));
        }

        let mut entries = Vec::new();
        let mut stack = vec![(root, 0_usize)];
        let mut truncated = false;

        while let Some((dir, level)) = stack.pop() {
            let metadata = fs::metadata(&dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect workspace path {}: {}",
                    dir.display(),
                    error
                ))
            })?;
            if metadata.is_file() {
                entries.push(WorkspaceEntry {
                    path: workspace_path_from_run_dir(&run_dir, &dir)?,
                    kind: WorkspaceEntryKind::File,
                });
                continue;
            }
            if !metadata.is_dir() {
                continue;
            }

            let mut children = fs::read_dir(&dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read workspace directory {}: {}",
                    dir.display(),
                    error
                ))
            })?;
            let mut child_paths = Vec::new();
            while let Some(entry) = children.next_entry().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read workspace directory entry {}: {}",
                    dir.display(),
                    error
                ))
            })? {
                child_paths.push(entry.path());
            }
            child_paths.sort();

            for child in child_paths.into_iter().rev() {
                if entries.len() >= max_entries {
                    truncated = true;
                    break;
                }

                let metadata = fs::symlink_metadata(&child).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to inspect workspace path {}: {}",
                        child.display(),
                        error
                    ))
                })?;
                if metadata.file_type().is_symlink() {
                    return Err(DomainError::InvalidData(format!(
                        "Workspace path targets a symlink: {}",
                        child.display()
                    )));
                }

                let path = workspace_path_from_run_dir(&run_dir, &child)?;
                if metadata.is_dir() {
                    entries.push(WorkspaceEntry {
                        path,
                        kind: WorkspaceEntryKind::Directory,
                    });
                    if level < depth {
                        stack.push((child, level + 1));
                    }
                } else if metadata.is_file() {
                    entries.push(WorkspaceEntry {
                        path,
                        kind: WorkspaceEntryKind::File,
                    });
                }
            }

            if truncated {
                break;
            }
        }

        entries.sort_by(|a, b| {
            let kind_order = match (&a.kind, &b.kind) {
                (WorkspaceEntryKind::Directory, WorkspaceEntryKind::File) => {
                    std::cmp::Ordering::Less
                }
                (WorkspaceEntryKind::File, WorkspaceEntryKind::Directory) => {
                    std::cmp::Ordering::Greater
                }
                _ => std::cmp::Ordering::Equal,
            };
            kind_order.then_with(|| a.path.as_str().cmp(b.path.as_str()))
        });

        Ok(WorkspaceFileList { entries, truncated })
    }

    async fn commit_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let _guard = self.persist_lock.lock().await;
        let changes = self.compute_persistent_changes(run_id).await?;
        self.commit_persistent_state(run_id, changes).await
    }
}

/// Reject calls that target an existing directory at `target`. The OS error
/// for `read_to_string` on a directory (EISDIR / "Is a directory") used to
/// bubble up as `DomainError::InternalError`, surfaced to the model as a
/// non-retryable `agent.internal_error`. The repository now returns a typed
/// domain error and leaves model-facing recovery advice to the tool layer.
async fn ensure_target_is_not_directory(
    target: &std::path::Path,
    workspace_path: &WorkspacePath,
) -> Result<(), DomainError> {
    match fs::symlink_metadata(target).await {
        Ok(metadata) if metadata.file_type().is_dir() => Err(
            DomainError::workspace_path_is_directory(workspace_path.as_str()),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to inspect workspace path {}: {}",
            target.display(),
            error
        ))),
    }
}

async fn verify_workspace_write_guard(
    target: &std::path::Path,
    workspace_path: &WorkspacePath,
    guard: WorkspaceWriteGuard,
) -> Result<(), DomainError> {
    match guard {
        WorkspaceWriteGuard::Unchecked => Ok(()),
        WorkspaceWriteGuard::MustNotExist => match read_existing_workspace_text(target).await? {
            Some(text) => {
                let current = workspace_file_from_text(workspace_path.clone(), text)?;
                Err(DomainError::workspace_write_conflict(
                    workspace_path.as_str(),
                    WorkspaceWriteConflictKind::AlreadyExists {
                        actual_sha256: current.sha256,
                    },
                ))
            }
            None => Ok(()),
        },
        WorkspaceWriteGuard::MustMatchSha256(expected_sha256) => {
            match read_existing_workspace_text(target).await? {
                Some(text) => {
                    let current = workspace_file_from_text(workspace_path.clone(), text)?;
                    if current.sha256 == expected_sha256 {
                        Ok(())
                    } else {
                        Err(DomainError::workspace_write_conflict(
                            workspace_path.as_str(),
                            WorkspaceWriteConflictKind::Stale {
                                expected_sha256,
                                actual_sha256: Some(current.sha256),
                            },
                        ))
                    }
                }
                None => Err(DomainError::workspace_write_conflict(
                    workspace_path.as_str(),
                    WorkspaceWriteConflictKind::Stale {
                        expected_sha256,
                        actual_sha256: None,
                    },
                )),
            }
        }
    }
}

async fn read_existing_workspace_text(
    target: &std::path::Path,
) -> Result<Option<String>, DomainError> {
    match fs::read_to_string(target).await {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to read workspace file {} for write guard: {}",
            target.display(),
            error
        ))),
    }
}
